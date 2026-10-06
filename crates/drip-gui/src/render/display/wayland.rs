//! Application-owned extended-linear BT.709 with an SDR Rec.2020 target \[11\].
//! Passthrough fixes reference white across drivers. Extended-target support is
//! required because wide-gamut colors have negative/above-one BT.709 components.

use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_v1::WpColorManagementSurfaceV1,
    wp_color_manager_v1::{
        self, Feature, Primaries, RenderIntent, TransferFunction, WpColorManagerV1,
    },
    wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1,
    wp_image_description_v1::{self, WpImageDescriptionV1},
};
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::window::Window;

/// Owns the window's color-management surface.
pub struct Description {
    surface: WpColorManagementSurfaceV1,
    _queue: EventQueue<State>,
}

impl Description {
    /// Describes `window`, which must be presented with pass-through. `None`
    /// if it is not a Wayland window or the compositor cannot describe it.
    pub fn new(window: &Window) -> Option<Self> {
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(handle)) =
            (window.display_handle().ok()?.as_raw(), window.window_handle().ok()?.as_raw())
        else {
            return None;
        };
        // SAFETY: winit's display stays connected while its windows exist, and a
        // backend made from a foreign display never disconnects it.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        // SAFETY: the surface belongs to `window`, which outlives this call.
        let id =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast()) };
        let target = WlSurface::from_id(&conn, id.ok()?).ok()?;

        let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let manager = globals.bind::<WpColorManagerV1, _, _>(&qh, 1..=1, ());
        globals.destroy();
        let manager = manager.ok()?;
        let surface = Self::attach(&manager, &target, &mut queue);
        manager.destroy();
        let description = Self { surface: surface?, _queue: queue };
        // Own the surface before flushing so a failed flush also destroys it.
        conn.flush().ok()?;
        Some(description)
    }

    fn attach(
        manager: &WpColorManagerV1,
        target: &WlSurface,
        queue: &mut EventQueue<State>,
    ) -> Option<WpColorManagementSurfaceV1> {
        let qh = queue.handle();
        let mut state = State::default();
        queue.roundtrip(&mut state).ok()?;
        let supported = [
            Feature::Parametric,
            Feature::SetLuminances,
            Feature::SetMasteringDisplayPrimaries,
            Feature::ExtendedTargetVolume,
        ]
        .iter()
        .all(|f| state.features.contains(f))
            && state.primaries.contains(&Primaries::Srgb)
            && state.transfers.contains(&TransferFunction::ExtLinear);
        if !supported {
            log::warn!(
                "the compositor cannot describe extended-linear sRGB with a Rec.2020 target"
            );
            return None;
        }

        let params = manager.create_parametric_creator(&qh, ());
        params.set_primaries_named(Primaries::Srgb);
        params.set_tf_named(TransferFunction::ExtLinear);
        // cd/m², min scaled by 10⁴. With a linear transfer, 1.0 is the maximum.
        params.set_luminances(0, 80, 80);
        let xy = |p: [f64; 2]| p.map(|v| (v * 1_000_000.0).round() as i32);
        let [[rx, ry], [gx, gy], [bx, by]] = drip::color::REC2020.map(xy);
        let [wx, wy] = xy(drip::color::D65);
        params.set_mastering_display_primaries(rx, ry, gx, gy, bx, by, wx, wy);
        let description = params.create(&qh, ());
        while state.ready.is_none() {
            if queue.blocking_dispatch(&mut state).is_err() {
                description.destroy();
                return None;
            }
        }
        let surface = (state.ready == Some(true)).then(|| {
            let intent = if state.intents.contains(&RenderIntent::Relative) {
                RenderIntent::Relative
            } else {
                RenderIntent::Perceptual
            };
            log::info!(
                "describing extended-linear sRGB, Rec.2020 target, SDR white 80 cd/m², {intent:?} intent"
            );
            let surface = manager.get_surface(target, &qh, ());
            surface.set_image_description(&description, intent);
            surface
        });
        description.destroy();
        surface
    }
}

impl Drop for Description {
    fn drop(&mut self) {
        self.surface.destroy();
    }
}

#[derive(Default)]
struct State {
    features: Vec<Feature>,
    primaries: Vec<Primaries>,
    transfers: Vec<TransferFunction>,
    intents: Vec<RenderIntent>,
    /// Whether the compositor accepted the description, once it answered.
    ready: Option<bool>,
}

impl Dispatch<WpColorManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpColorManagerV1,
        event: wp_color_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wp_color_manager_v1::Event as E;
        match event {
            E::SupportedFeature { feature: WEnum::Value(f) } => state.features.push(f),
            E::SupportedPrimariesNamed { primaries: WEnum::Value(p) } => state.primaries.push(p),
            E::SupportedTfNamed { tf: WEnum::Value(t) } => state.transfers.push(t),
            E::SupportedIntent { render_intent: WEnum::Value(i) } => state.intents.push(i),
            _ => {}
        }
    }
}

impl Dispatch<WpImageDescriptionV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpImageDescriptionV1,
        event: wp_image_description_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wp_image_description_v1::Event as E;
        match event {
            E::Ready { .. } | E::Ready2 { .. } => state.ready = Some(true),
            E::Failed { msg, .. } => {
                log::warn!("the compositor rejected the surface description: {msg}");
                state.ready = Some(false);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore WpImageDescriptionCreatorParamsV1);
delegate_noop!(State: ignore WpColorManagementSurfaceV1);
