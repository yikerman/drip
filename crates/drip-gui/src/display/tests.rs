use super::{CANVAS, Output};
use wgpu::{SurfaceCapabilities, SurfaceColorSpace as Space, SurfaceColorSpaces as Spaces};

fn capabilities(formats: &[(wgpu::TextureFormat, Spaces)]) -> SurfaceCapabilities {
    SurfaceCapabilities {
        format_capabilities: formats
            .iter()
            .map(|&(format, color_spaces)| wgpu::SurfaceFormatCapabilities { format, color_spaces })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn select_supported_format_and_encoding_together() {
    use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgb10a2Unorm};
    let caps = capabilities(&[
        (Rgb10a2Unorm, Spaces::BT2100_PQ),
        (Bgra8UnormSrgb, Spaces::SRGB),
        (Bgra8Unorm, Spaces::SRGB),
        (CANVAS, Spaces::EXTENDED_SRGB_LINEAR),
    ]);
    let output = Output::choose(&caps).unwrap();
    assert_eq!(output.color_space, Space::ExtendedSrgbLinear);
    assert_eq!(output.format, CANVAS);
    assert!(output.wide_gamut());

    let caps = capabilities(
        &caps.format_capabilities[..3]
            .iter()
            .map(|f| (f.format, f.color_spaces))
            .collect::<Vec<_>>(),
    );
    let output = Output::choose(&caps).unwrap();
    assert_eq!(output.color_space, Space::Srgb);
    assert_eq!(output.format, Bgra8Unorm);
    assert!(!output.wide_gamut());

    let output = Output::choose(&capabilities(&[(Bgra8UnormSrgb, Spaces::SRGB)])).unwrap();
    assert_eq!(output.fragment(), "srgb_linear");
    assert!(Output::choose(&capabilities(&[(CANVAS, Spaces::EXTENDED_SRGB)])).is_none());
    assert!(Output::choose(&capabilities(&[(Rgb10a2Unorm, Spaces::BT2100_PQ)])).is_none());
}

// This checks the real preview + composite shaders, including FP16 storage.
// It intentionally fails if no adapter is available when explicitly invoked.
#[test]
#[ignore = "requires a GPU adapter; run with --ignored"]
fn gpu_preview_and_output_match_colorimetric_reference() {
    use drip::{color, profile};
    use lcms2::{Intent, PixelFormat, Transform};

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    eprintln!("display shader test: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let rgb = [
        [0.0; 3],
        [0.003; 3],
        [0.018; 3],
        [0.18; 3],
        [0.5; 3],
        [1.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [-0.02, 0.1, 0.3],
        [1.2, 0.2, 0.4],
        [0.05, 0.3, 0.1],
        [0.2, 0.05, 0.01],
        [2.0; 3],
        [-0.01; 3],
    ];
    let width = rgb.len() as u32;
    let source = texture(&device, width, CANVAS);
    let bytes: Vec<u8> = rgb
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 1.0])
        .flat_map(|v| half::f16::from_f32(v).to_ne_bytes())
        .collect();
    queue.write_texture(
        source.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 8),
            rows_per_image: None,
        },
        source.size(),
    );
    let shader = device.create_shader_module(wgpu::include_wgsl!("../shaders/preview.wgsl"));
    let pipeline = create_pipeline(&device, &shader, "fs", CANVAS);
    let source_view = source.create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let rect = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&rect, 0, &[-1.0f32, 1.0, 1.0, -1.0].map(f32::to_ne_bytes).concat());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: rect.as_entire_binding() },
        ],
    });
    let canvas = texture(&device, width, CANVAS);
    draw(&device, &queue, &pipeline, &group, &canvas, 6);
    let canvas_view = canvas.create_view(&Default::default());
    let shader = device.create_shader_module(wgpu::include_wgsl!("../shaders/composite.wgsl"));
    let source_profile = profile::rec2020_linear();
    let destination_profile = profile::built_in("srgb");
    let convert = Transform::new(
        &source_profile,
        PixelFormat::RGB_FLT,
        &destination_profile,
        PixelFormat::RGB_16,
        Intent::RelativeColorimetric,
    )
    .unwrap();
    let mut reference = vec![[0u16; 3]; rgb.len()];
    convert.transform_pixels(&rgb, &mut reference);
    let matrix = color::mul(
        &color::inverse(&color::rgb_to_xyz(color::REC709, color::D65)),
        &color::rgb_to_xyz(color::REC2020, color::D65),
    );
    for output in [
        Output { format: CANVAS, color_space: Space::ExtendedSrgbLinear },
        Output { format: wgpu::TextureFormat::Rgba8Unorm, color_space: Space::Srgb },
        Output { format: wgpu::TextureFormat::Rgba8UnormSrgb, color_space: Space::Srgb },
    ] {
        let pipeline = create_pipeline(&device, &shader, output.fragment(), output.format);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&canvas_view),
            }],
        });
        let target = texture(&device, width, output.format);
        draw(&device, &queue, &pipeline, &group, &target, 3);
        let bytes = read(&device, &queue, &target);
        for (i, pixel) in rgb.iter().enumerate() {
            if output.wide_gamut() {
                let actual: Vec<f64> = bytes[i * 8..i * 8 + 6]
                    .chunks_exact(2)
                    .map(|b| half::f16::from_ne_bytes([b[0], b[1]]).to_f64())
                    .collect();
                let expected = color::apply(&matrix, pixel.map(f64::from));
                for channel in 0..3 {
                    assert!(
                        (actual[channel] - expected[channel]).abs() < 0.004,
                        "{pixel:?}: {actual:?} != {expected:?}"
                    );
                }
            } else {
                for channel in 0..3 {
                    let expected = (f64::from(reference[i][channel]) / 257.0).round() as i32;
                    let actual = i32::from(bytes[i * 4 + channel]);
                    assert!(
                        (actual - expected).abs() <= 1,
                        "{output:?}, {pixel:?}, channel {channel}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

fn texture(device: &wgpu::Device, width: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    fragment: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            targets: &[Some(format.into())],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    target: &wgpu::Texture,
    vertices: u32,
) {
    let view = target.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..vertices, 0..1);
    }
    queue.submit([encoder.finish()]);
}

fn read(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| tx.send(r).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    buffer.get_mapped_range(..).unwrap().to_vec()
}
