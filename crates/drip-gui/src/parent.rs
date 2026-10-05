//! Pop-outs as children of the main window: the window manager
//! keeps a child above its parent and minimizes it with it. winit has no
//! parent windows on Wayland, so Drip sends `xdg_toplevel.set_parent` itself,
//! on winit's connection. Other platforms are not handled yet (TODO).

use winit::window::Window;

/// Makes `child` a child of `parent`.
#[cfg(target_os = "linux")]
pub fn set_parent(child: &Window, parent: &Window) {
    use wayland_client::backend::{Backend, ObjectId};
    use wayland_client::{Connection, Proxy};
    use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
    use winit::platform::wayland::WindowExtWayland;
    use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

    let Ok(RawDisplayHandle::Wayland(display)) = child.display_handle().map(|h| h.as_raw()) else {
        return;
    };
    // SAFETY: winit's display stays connected while its windows exist, and a
    // backend made from a foreign display never disconnects it.
    let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
    let conn = Connection::from_backend(backend);
    let toplevel = |window: &Window| {
        let proxy = window.xdg_toplevel()?.as_ptr().cast();
        // SAFETY: the proxy belongs to `window`, which outlives this call.
        let id = unsafe { ObjectId::from_ptr(XdgToplevel::interface(), proxy) }.ok()?;
        XdgToplevel::from_id(&conn, id).ok()
    };
    if let (Some(child), Some(parent)) = (toplevel(child), toplevel(parent)) {
        child.set_parent(Some(&parent));
        if let Err(e) = conn.flush() {
            log::warn!("cannot keep a window above the main window: {e}");
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn set_parent(_: &Window, _: &Window) {}
