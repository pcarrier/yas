//! Cursor authority follows pointer focus, and a cursor surface commits far
//! more often than its artwork changes.
//!
//! A stale enter serial must not let the previous client overwrite the current
//! surface, and selecting a named shape must retire the prior cursor surface.
//! Separately, Xwayland re-attaches its cursor on pointer enter and on every
//! update it was throttling behind a frame callback, so the same image is
//! committed again and again. Announcing it every time is not merely wasted
//! bandwidth: a viewer that rebuilds an object URL per announcement revokes
//! the one it is drawing from, and the cursor blinks at whatever rate the
//! commits arrive.

#![cfg(target_os = "linux")]

use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1, wp_cursor_shape_manager_v1,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use yas_compositor::{
    CompositorCommand, CompositorEvent, CursorImage, spawn_compositor_without_renderer,
};

#[derive(Default)]
struct App {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    cursor_shape_manager: Option<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1>,
    enter_serial: Option<u32>,
    leave_count: usize,
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor =
                        Some(registry.bind::<wl_compositor::WlCompositor, _, _>(name, 4, qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ()));
                }
                "wl_seat" => {
                    state.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(name, 5, qh, ()));
                }
                "xdg_wm_base" => {
                    state.wm_base =
                        Some(registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, 1, qh, ()));
                }
                "wp_cursor_shape_manager_v1" => {
                    state.cursor_shape_manager = Some(
                        registry.bind::<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1, _, _>(
                            name,
                            1,
                            qh,
                            (),
                        ),
                    );
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(App: ignore wl_compositor::WlCompositor);
delegate_noop!(App: ignore wl_surface::WlSurface);
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore wl_buffer::WlBuffer);
delegate_noop!(App: ignore wl_seat::WlSeat);
delegate_noop!(App: ignore wp_cursor_shape_manager_v1::WpCursorShapeManagerV1);
delegate_noop!(App: ignore wp_cursor_shape_device_v1::WpCursorShapeDeviceV1);

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { serial, .. } => state.enter_serial = Some(serial),
            wl_pointer::Event::Leave { .. } => state.leave_count += 1,
            _ => {}
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(
        _: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for App {
    fn event(
        _: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        _: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Cursor requests need current focus authority, replace one another, and do
/// not re-announce byte-identical surface artwork.
#[test]
fn cursor_authority_replacement_and_artwork_deduplication() {
    let notify_count = Arc::new(AtomicUsize::new(0));
    let notify_count_for_compositor = notify_count.clone();
    let handle = spawn_compositor_without_renderer(
        false,
        Arc::new(move || {
            notify_count_for_compositor.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let stream = UnixStream::connect(&handle.socket_name).expect("connect to compositor socket");
    let conn = Connection::from_socket(stream).expect("wayland connection");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());

    let mut app = App::default();
    queue.roundtrip(&mut app).expect("registry roundtrip");
    let compositor = app.compositor.clone().expect("wl_compositor");
    let shm = app.shm.clone().expect("wl_shm");
    let seat = app.seat.clone().expect("wl_seat");
    let wm_base = app.wm_base.clone().expect("xdg_wm_base");
    let cursor_shape_manager = app
        .cursor_shape_manager
        .clone()
        .expect("wp_cursor_shape_manager_v1");
    let pointer = seat.get_pointer(&qh, ());
    let cursor_shape = cursor_shape_manager.get_pointer(&pointer, &qh, ());

    // Map a real pointer target. Cursor requests without a matching enter are
    // invalid Wayland and must be ignored by the compositor.
    let root = compositor.create_surface(&qh, ());
    let xdg_surface = wm_base.get_xdg_surface(&root, &qh, ());
    let _toplevel = xdg_surface.get_toplevel(&qh, ());
    root.commit();
    queue.roundtrip(&mut app).expect("configure roundtrip");

    // A 4x4 opaque cursor in a shm pool.
    let (w, h) = (4i32, 4i32);
    let stride = w * 4;
    let size = stride * h;
    let root_w = 64i32;
    let root_h = 64i32;
    let root_size = root_w * root_h * 4;
    // An unlinked file under the runtime dir: the compositor only needs a
    // readable fd of the right length, and this avoids a dev-dependency.
    let path = std::env::temp_dir().join(format!("yas-cursor-test-{}", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .expect("shm backing file");
    std::fs::remove_file(&path).ok();
    file.set_len((size + root_size) as u64)
        .expect("size shm file");
    let pool = shm.create_pool(std::os::fd::AsFd::as_fd(&file), size + root_size, &qh, ());
    let buffer = pool.create_buffer(0, w, h, stride, wl_shm::Format::Argb8888, &qh, ());

    let root_buffer = pool.create_buffer(
        size,
        root_w,
        root_h,
        root_w * 4,
        wl_shm::Format::Xrgb8888,
        &qh,
        (),
    );
    root.attach(Some(&root_buffer), 0, 0);
    root.damage_buffer(0, 0, root_w, root_h);
    root.commit();
    queue.roundtrip(&mut app).expect("map roundtrip");

    let surface_id = loop {
        match handle.event_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(CompositorEvent::SurfaceCreated { surface_id, .. }) => break surface_id,
            Ok(_) => {}
            Err(error) => panic!("surface was not created: {error}"),
        }
    };
    handle
        .command_tx
        .send(CompositorCommand::PointerMotion {
            surface_id,
            x: 10.0,
            y: 10.0,
            time_ms: 0,
        })
        .expect("send pointer motion");
    handle.wake();
    std::thread::sleep(Duration::from_millis(50));
    queue.roundtrip(&mut app).expect("pointer enter roundtrip");
    let enter_serial = app.enter_serial.expect("pointer enter serial");

    let cursor = compositor.create_surface(&qh, ());
    pointer.set_cursor(enter_serial, Some(&cursor), 1, 1);
    // Five commits of byte-identical artwork.
    for _ in 0..5 {
        cursor.attach(Some(&buffer), 0, 0);
        cursor.damage(0, 0, w, h);
        cursor.commit();
        queue.roundtrip(&mut app).expect("cursor commit roundtrip");
    }

    // An old enter serial cannot change the cursor, even when the request is
    // otherwise well-formed. A late request from the surface just left is the
    // cross-client race that used to overwrite the new surface's shape.
    cursor_shape.set_shape(
        enter_serial.wrapping_sub(1),
        wp_cursor_shape_device_v1::Shape::Text,
    );
    pointer.set_cursor(enter_serial.wrapping_sub(1), None, 0, 0);
    queue
        .roundtrip(&mut app)
        .expect("stale cursor requests roundtrip");

    // A current named shape replaces the cursor surface. Its later animation
    // commit must not replace the named shape in turn unless set_cursor makes
    // that surface current again.
    let notifications_before_shape = notify_count.load(Ordering::Relaxed);
    cursor_shape.set_shape(enter_serial, wp_cursor_shape_device_v1::Shape::Text);
    queue.roundtrip(&mut app).expect("named shape roundtrip");
    assert!(
        notify_count.load(Ordering::Relaxed) > notifications_before_shape,
        "a cursor-only shape change did not wake the event consumer"
    );
    cursor.attach(Some(&buffer), 0, 0);
    cursor.damage(0, 0, w, h);
    cursor.commit();
    queue
        .roundtrip(&mut app)
        .expect("retired cursor surface roundtrip");

    // Re-selecting a pooled surface uses the pixels it already committed;
    // neither selection nor a hotspot-only update needs another surface
    // commit to become visible.
    pointer.set_cursor(enter_serial, Some(&cursor), 1, 1);
    queue
        .roundtrip(&mut app)
        .expect("pooled cursor surface restore roundtrip");
    pointer.set_cursor(enter_serial, Some(&cursor), 2, 3);
    queue.roundtrip(&mut app).expect("hotspot update roundtrip");

    // Hiding then restoring the same pooled cursor is a real state change in
    // both directions. The restored image must not be suppressed merely
    // because it matches the image used before the hide.
    pointer.set_cursor(enter_serial, None, 0, 0);
    queue.roundtrip(&mut app).expect("cursor hide roundtrip");
    pointer.set_cursor(enter_serial, Some(&cursor), 1, 1);
    queue.roundtrip(&mut app).expect("cursor restore roundtrip");

    // Browser `mouseleave` must reach Wayland. Otherwise a hidden cursor is
    // permanent on this toplevel: motion returning to the same surface sees
    // unchanged compositor focus, emits no new enter, and gives the client no
    // serial with which to restore its cursor.
    pointer.set_cursor(enter_serial, None, 0, 0);
    queue.roundtrip(&mut app).expect("cursor re-hide roundtrip");
    handle
        .command_tx
        .send(CompositorCommand::PointerLeave { surface_id })
        .expect("send pointer leave");
    handle.wake();
    std::thread::sleep(Duration::from_millis(50));
    queue.roundtrip(&mut app).expect("pointer leave roundtrip");
    assert_eq!(app.leave_count, 1, "browser leave did not reach Wayland");

    handle
        .command_tx
        .send(CompositorCommand::PointerMotion {
            surface_id,
            x: 10.0,
            y: 10.0,
            time_ms: 0,
        })
        .expect("send returning pointer motion");
    handle.wake();
    std::thread::sleep(Duration::from_millis(50));
    queue
        .roundtrip(&mut app)
        .expect("pointer re-enter roundtrip");
    let reenter_serial = app.enter_serial.expect("pointer re-enter serial");
    assert_ne!(
        reenter_serial, enter_serial,
        "returning to the same surface did not produce a fresh enter"
    );
    cursor_shape.set_shape(reenter_serial, wp_cursor_shape_device_v1::Shape::Default);
    queue
        .roundtrip(&mut app)
        .expect("cursor restored after re-enter");

    let mut announcements = Vec::new();
    while let Ok(event) = handle.event_rx.recv_timeout(Duration::from_millis(300)) {
        if let CompositorEvent::SurfaceCursor { cursor, .. } = event {
            announcements.push(match cursor {
                CursorImage::Named(name) => name,
                CursorImage::Custom {
                    hotspot_x,
                    hotspot_y,
                    ..
                } => format!("custom:{hotspot_x}:{hotspot_y}"),
                CursorImage::Hidden => "hidden".to_string(),
            });
        }
    }
    assert_eq!(
        announcements,
        [
            "custom:1:1",
            "text",
            "custom:1:1",
            "custom:2:3",
            "hidden",
            "custom:1:1",
            "hidden",
            "default",
        ],
        "a stale shape, identical commits, and commits from a replaced cursor \
         surface must all remain silent, while leave/re-enter restores a hidden cursor",
    );
    handle.stop();
}
