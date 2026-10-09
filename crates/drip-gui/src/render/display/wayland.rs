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
    /// Describes `window`, which must be presented with pass-through. Failure
    /// reasons travel to presentation selection for one fallback warning.
    pub fn new(window: &Window) -> Result<Self, String> {
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(handle)) = (
            window.display_handle().map_err(|e| e.to_string())?.as_raw(),
            window.window_handle().map_err(|e| e.to_string())?.as_raw(),
        ) else {
            return Err("not a Wayland window".into());
        };
        // SAFETY: winit's display stays connected while its windows exist, and a
        // backend made from a foreign display never disconnects it.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        // SAFETY: the surface belongs to `window`, which outlives this call.
        let id =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast()) };
        let target =
            WlSurface::from_id(&conn, id.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

        let (globals, mut queue) = registry_queue_init::<State>(&conn)
            .map_err(|e| format!("color-management registry: {e}"))?;
        let qh = queue.handle();
        let manager = globals.bind::<WpColorManagerV1, _, _>(&qh, 1..=1, ());
        globals.destroy();
        let manager = manager.map_err(|e| format!("color-management protocol: {e}"))?;
        let surface = Self::attach(&manager, &target, &mut queue);
        manager.destroy();
        let (surface, intent) = surface?;
        let description = Self { surface, _queue: queue };
        // Own the surface before flushing so a failed flush also destroys it.
        conn.flush().map_err(|e| format!("flush surface description: {e}"))?;
        log::debug!(
            "window={:?} description=extended-linear-sRGB target=Rec.2020 white=80cd/m² intent={intent:?}",
            window.id()
        );
        Ok(description)
    }

    fn attach(
        manager: &WpColorManagerV1,
        target: &WlSurface,
        queue: &mut EventQueue<State>,
    ) -> Result<(WpColorManagementSurfaceV1, RenderIntent), String> {
        let qh = queue.handle();
        let mut state = State::default();
        queue.roundtrip(&mut state).map_err(|e| format!("read color capabilities: {e}"))?;
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
            return Err(
                "compositor cannot describe extended-linear sRGB with a Rec.2020 target".into()
            );
        }

        let params = manager.create_parametric_creator(&qh, ());
        params.set_primaries_named(Primaries::Srgb);
        params.set_tf_named(TransferFunction::ExtLinear);
        // cd/m², min scaled by 10⁴. With a linear transfer, 1.0 is the maximum.
        params.set_luminances(0, 80, 80);
        let xy = |p: [f64; 2]| p.map(|v| (v * 1_000_000.0).round() as i32);
        let [[rx, ry], [gx, gy], [bx, by]] = drip::node::color::REC2020.map(xy);
        let [wx, wy] = xy(drip::node::color::D65);
        params.set_mastering_display_primaries(rx, ry, gx, gy, bx, by, wx, wy);
        let description = params.create(&qh, ());
        while state.ready.is_none() {
            if let Err(error) = queue.blocking_dispatch(&mut state) {
                description.destroy();
                return Err(format!("receive surface description: {error}"));
            }
        }
        let surface = state.ready.expect("description answered").map(|()| {
            let intent = if state.intents.contains(&RenderIntent::Relative) {
                RenderIntent::Relative
            } else {
                RenderIntent::Perceptual
            };
            let surface = manager.get_surface(target, &qh, ());
            surface.set_image_description(&description, intent);
            (surface, intent)
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
    /// The compositor's answer, retaining rejection details.
    ready: Option<Result<(), String>>,
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
            E::Ready { .. } | E::Ready2 { .. } => state.ready = Some(Ok(())),
            E::Failed { msg, .. } => {
                state.ready = Some(Err(format!("compositor rejected surface description: {msg}")));
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
