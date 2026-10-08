//! Ordered WGSL computation and explicit host/device transfers.
//!
//! Submissions share one queue: a consumer observes earlier producers without a
//! host wait. Only readback and explicit completion wait for the device. Buffers
//! remain owned by wgpu while submitted commands use them.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use wgpu::util::DeviceExt;

static NEXT_DEVICE: AtomicU64 = AtomicU64::new(1);
const DEFAULT_MEMORY_BUDGET: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ComputeError {
    #[error("no usable compute adapter: {0}")]
    Adapter(String),
    #[error("cannot create compute device: {0}")]
    Device(String),
    #[error("buffer has {bytes} bytes; this device permits {limit} bytes per storage binding")]
    BufferLimit { bytes: u64, limit: u64 },
    #[error(
        "GPU memory budget exhausted: requested {requested} bytes with {used}/{budget} bytes live"
    )]
    MemoryBudget { requested: u64, used: u64, budget: u64 },
    #[error("GPU buffer belongs to another compute context")]
    WrongDevice,
    #[error("invalid compute request: {0}")]
    Invalid(String),
    #[error("GPU completion/readback failed: {0}")]
    Readback(String),
}

/// Shared, packed 32-bit storage. Interpretation belongs to the payload using it.
/// No host sample accessor is provided; readback is an explicit operation.
#[derive(Clone, Debug)]
pub struct GpuBuffer {
    buffer: wgpu::Buffer,
    len: usize,
    device: u64,
    _reservation: Arc<Reservation>,
}
impl GpuBuffer {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn raw(&self) -> &wgpu::Buffer {
        &self.buffer
    }
}

struct Pipeline {
    source: String,
    entry: String,
    pipeline: wgpu::ComputePipeline,
}

#[derive(Debug)]
struct MemoryBudget {
    used: AtomicU64,
    limit: u64,
}
#[derive(Debug)]
struct Reservation {
    memory: Arc<MemoryBudget>,
    bytes: u64,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.memory.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Device resources, compiled pipelines, and transfer counters; no image cache.
/// Cloning the `Arc` shares the queue and does not create another logical device.
pub struct Compute {
    device: wgpu::Device,
    queue: wgpu::Queue,
    id: u64,
    pipelines: Mutex<HashMap<&'static str, Pipeline>>,
    uploads: AtomicU64,
    downloads: AtomicU64,
    memory: Arc<MemoryBudget>,
}
impl std::fmt::Debug for Compute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compute").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Drop for Compute {
    fn drop(&mut self) {
        // Deliver pending retirement callbacks before the last runtime owner
        // disappears. This is a shutdown boundary, not a per-kernel wait.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

impl Compute {
    /// Prefer a hardware adapter; retry with a software adapter when necessary.
    /// Software execution uses the same WGSL kernels (for example Mesa lavapipe).
    pub fn new() -> Result<Arc<Self>, ComputeError> {
        Self::new_with_budget(DEFAULT_MEMORY_BUDGET)
    }
    /// Bound sample buffers and readback staging independently of device limits.
    /// Driver allocations, compiled pipelines and frontend textures are outside
    /// this budget and must be allowed for by the application's chosen limit.
    pub fn new_with_budget(budget: u64) -> Result<Arc<Self>, ComputeError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let preferred = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        };
        let adapter = pollster::block_on(instance.request_adapter(&preferred))
            .or_else(|_| {
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    force_fallback_adapter: true,
                    ..preferred
                }))
            })
            .map_err(|error| ComputeError::Adapter(error.to_string()))?;
        log::info!("Compute adapter: {:?}", adapter.get_info());
        #[cfg(test)]
        eprintln!("Compute adapter: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Drip compute"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|error| ComputeError::Device(error.to_string()))?;
        Ok(Self::from_device_with_budget(device, queue, budget))
    }

    /// Share a frontend device so resident image buffers can reach presentation
    /// without crossing device boundaries. The queue must belong to this device.
    pub fn from_device(device: wgpu::Device, queue: wgpu::Queue) -> Arc<Self> {
        Self::from_device_with_budget(device, queue, DEFAULT_MEMORY_BUDGET)
    }
    pub fn from_device_with_budget(
        device: wgpu::Device,
        queue: wgpu::Queue,
        budget: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            device,
            queue,
            id: NEXT_DEVICE.fetch_add(1, Ordering::Relaxed),
            pipelines: Mutex::new(HashMap::new()),
            uploads: AtomicU64::new(0),
            downloads: AtomicU64::new(0),
            memory: Arc::new(MemoryBudget { used: AtomicU64::new(0), limit: budget }),
        })
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn memory_usage(&self) -> (u64, u64) {
        (self.memory.used.load(Ordering::Acquire), self.memory.limit)
    }
    fn reserve(&self, bytes: u64) -> Result<Arc<Reservation>, ComputeError> {
        // Reap completion callbacks without turning each kernel into a host wait.
        let _ = self.device.poll(wgpu::PollType::Poll);
        let claim = || {
            self.memory.used.try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|&n| n <= self.memory.limit)
            })
        };
        if claim().is_err() {
            // Graph handles can disappear before their submissions finish.
            // Under pressure only, retire that work before deciding the live
            // working set exceeds the budget. No result cache is evicted.
            self.finish()?;
            claim().map_err(|used| ComputeError::MemoryBudget {
                requested: bytes,
                used,
                budget: self.memory.limit,
            })?;
        }
        Ok(Arc::new(Reservation { memory: self.memory.clone(), bytes }))
    }
    /// Number of explicit sample uploads and downloads since context creation.
    pub fn transfer_counts(&self) -> (u64, u64) {
        (self.uploads.load(Ordering::Relaxed), self.downloads.load(Ordering::Relaxed))
    }
    fn bytes(&self, len: usize) -> Result<u64, ComputeError> {
        let bytes = u64::try_from(len)
            .ok()
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| ComputeError::Invalid("sample count overflow".into()))?;
        let limits = self.device.limits();
        let limit = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size);
        if bytes > limit {
            return Err(ComputeError::BufferLimit { bytes, limit });
        }
        if bytes == 0 {
            return Err(ComputeError::Invalid("empty storage buffer".into()));
        }
        Ok(bytes)
    }
    pub fn allocate_f32(&self, len: usize) -> Result<GpuBuffer, ComputeError> {
        let size = self.bytes(len)?;
        let reservation = self.reserve(size)?;
        Ok(GpuBuffer {
            buffer: self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Drip samples"),
                size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            len,
            device: self.id,
            _reservation: reservation,
        })
    }
    pub fn upload_f32(&self, samples: &[f32]) -> Result<GpuBuffer, ComputeError> {
        let size = self.bytes(samples.len())?;
        let reservation = self.reserve(size)?;
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Drip upload"),
            contents: bytemuck::cast_slice(samples),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });
        self.uploads.fetch_add(1, Ordering::Relaxed);
        Ok(GpuBuffer { buffer, len: samples.len(), device: self.id, _reservation: reservation })
    }
    pub fn check_buffer(&self, buffer: &GpuBuffer) -> Result<(), ComputeError> {
        if buffer.device != self.id {
            return Err(ComputeError::WrongDevice);
        }
        Ok(())
    }
    /// Dispatch one shader with consecutive storage bindings in group zero.
    /// WGSL declares read/read-write access. A key names one source/entry pair;
    /// accidental reuse for another shader is rejected rather than miscompiled.
    pub fn dispatch(
        &self,
        key: &'static str,
        source: &str,
        entry: &str,
        bindings: &[&GpuBuffer],
        groups: [u32; 3],
    ) -> Result<(), ComputeError> {
        for buffer in bindings {
            self.check_buffer(buffer)?;
        }
        let limit = self.device.limits().max_compute_workgroups_per_dimension;
        if groups.iter().any(|&n| n > limit) {
            return Err(ComputeError::Invalid(format!("workgroup dimensions exceed {limit}")));
        }
        let mut pipelines = self.pipelines.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = pipelines.get(key) {
            if previous.source != source || previous.entry != entry {
                return Err(ComputeError::Invalid(format!("shader key {key} reused")));
            }
        } else {
            let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(key),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(key),
                layout: None,
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            });
            pipelines
                .insert(key, Pipeline { source: source.into(), entry: entry.into(), pipeline });
        }
        let pipeline = &pipelines[key].pipeline;
        let entries: Vec<_> = bindings
            .iter()
            .enumerate()
            .map(|(i, buffer)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: buffer.buffer.as_entire_binding(),
            })
            .collect();
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(key),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(key) });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(key),
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
        }
        self.queue.submit([encoder.finish()]);
        // Graph handles may die at last use, but submitted device work still
        // owns these allocations. Keep their budget reservations until completion.
        let submitted: Vec<GpuBuffer> = bindings.iter().map(|b| (*b).clone()).collect();
        self.queue.on_submitted_work_done(move || drop(submitted));
        Ok(())
    }
    /// Copy to staging memory and wait once at a host consumer boundary.
    pub fn read_f32(&self, input: &GpuBuffer) -> Result<Vec<f32>, ComputeError> {
        self.check_buffer(input)?;
        let size = self.bytes(input.len)?;
        let _staging_reservation = self.reserve(size)?;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Drip readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&input.buffer, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.finish()?;
        receiver
            .recv()
            .map_err(|e| ComputeError::Readback(e.to_string()))?
            .map_err(|e| ComputeError::Readback(e.to_string()))?;
        let mapped = slice.get_mapped_range().map_err(|e| ComputeError::Readback(e.to_string()))?;
        let result = bytemuck::cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        staging.unmap();
        self.downloads.fetch_add(1, Ordering::Relaxed);
        Ok(result)
    }
    pub fn finish(&self) -> Result<(), ComputeError> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| ComputeError::Readback(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resident_wgsl_chain_and_explicit_readback() {
        let compute = Compute::new().expect("hardware or software compute adapter required");
        let input = compute.upload_f32(&[-1.0, 0.25, 2.0, 8.0]).unwrap();
        let intermediate = compute.allocate_f32(4).unwrap();
        let output = compute.allocate_f32(4).unwrap();
        let shader = r#"
            @group(0) @binding(0) var<storage, read> input: array<f32>;
            @group(0) @binding(1) var<storage, read_write> output: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                if id.x < arrayLength(&input) { output[id.x] = input[id.x] * 2.0; }
            }
        "#;
        compute.dispatch("test gain", shader, "main", &[&input, &intermediate], [1, 1, 1]).unwrap();
        compute
            .dispatch("test gain", shader, "main", &[&intermediate, &output], [1, 1, 1])
            .unwrap();
        assert_eq!(compute.transfer_counts(), (1, 0));
        assert_eq!(compute.read_f32(&output).unwrap(), [-4.0, 1.0, 8.0, 32.0]);
        assert_eq!(compute.transfer_counts(), (1, 1));
        assert!(compute.dispatch("test gain", "different shader", "main", &[], [1, 1, 1]).is_err());
        assert!(compute.allocate_f32(usize::MAX).is_err());
        assert!(compute.allocate_f32(0).is_err());
        let bounded =
            Compute::from_device_with_budget(compute.device().clone(), compute.queue().clone(), 16);
        let buffer = bounded.allocate_f32(4).unwrap();
        let alias = buffer.clone();
        assert_eq!(bounded.memory_usage(), (16, 16));
        assert!(matches!(bounded.allocate_f32(1), Err(ComputeError::MemoryBudget { .. })));
        assert!(matches!(bounded.read_f32(&buffer), Err(ComputeError::MemoryBudget { .. })));
        assert!(matches!(compute.read_f32(&buffer), Err(ComputeError::WrongDevice)));
        drop(buffer);
        assert_eq!(bounded.memory_usage(), (16, 16));
        drop(alias);
        assert_eq!(bounded.memory_usage(), (0, 16));

        // Two buffers fill the budget. The input's last graph owner disappears
        // after dispatch; a new allocation must wait for retirement if needed,
        // rather than report exhaustion from that queued reference alone.
        let pressure =
            Compute::from_device_with_budget(compute.device().clone(), compute.queue().clone(), 32);
        let retired = pressure.upload_f32(&[1.0, 2.0, 3.0, 4.0]).unwrap();
        let retained = pressure.allocate_f32(4).unwrap();
        pressure
            .dispatch("budget gain", shader, "main", &[&retired, &retained], [1, 1, 1])
            .unwrap();
        drop(retired);
        let replacement =
            pressure.allocate_f32(4).expect("submitted-only input must retire under pressure");
        assert_eq!(pressure.memory_usage(), (32, 32));
        assert!(matches!(pressure.allocate_f32(1), Err(ComputeError::MemoryBudget { .. })));
        drop(replacement);
        assert_eq!(pressure.read_f32(&retained).unwrap(), [2.0, 4.0, 6.0, 8.0]);
        drop(retained);
        assert_eq!(pressure.memory_usage(), (0, 32));
    }
}
