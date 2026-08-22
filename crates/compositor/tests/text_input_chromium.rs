//! The same contract, but judged by a real toolkit instead of by us.
//!
//! `text_input.rs` drives a client we wrote, so it proves we send what we
//! meant to send -- not that anything accepts it.  Serial semantics, the
//! double-buffered `enable`, and the requirement that `done` follow
//! `commit_string` are all rules a client is free to enforce, and a hand-
//! written client that ignores them cannot tell us we got them wrong.
//!
//! Chromium implements `zwp_text_input_v3` for real, so it is the judge.  The
//! page echoes whatever lands in its focused field back into `document.title`,
//! which Chromium turns into `xdg_toplevel.set_title` -- so the answer comes
//! back over the same Wayland connection, with no debugger attached.
//!
//! Ignored by default: it starts a browser.  Run it with
//! `cargo test -p yas-compositor --test text_input_chromium -- --ignored`.
//! Set `YAS_TEST_CHROMIUM` to use another Chromium executable, such as Brave.

#![cfg(target_os = "linux")]

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use yas_compositor::{CompositorCommand, CompositorEvent, spawn_compositor};

/// ASCII first, then the part that has no key.  One string, so the reply
/// distinguishes the three outcomes that matter: `hi日本語` is the fix
/// working, a bare `hi` is the composed half still being dropped, and no
/// title at all is a harness that never got the field focused.
const TYPED: &str = "hi日本語";

/// What a composition in progress should look like on the way through.
const COMPOSING: &str = "にほn";

const PAGE: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>waiting</title>
<input id="f" autofocus>
<script>
  const f = document.getElementById('f');
  f.focus();
  // A composition shows up in `value` like anything else, so the title has
  // to say which one it is — otherwise the `input` that follows every
  // compositionupdate overwrites the pending text with the same string
  // under the committed label.
  let composing = false;
  f.addEventListener('compositionstart', () => { composing = true; });
  f.addEventListener('compositionend', () => { composing = false; });
  f.addEventListener('input', () => {
    document.title = (composing ? 'PRE:' : 'GOT:') + f.value;
  });
</script>
"#;

/// A field that moves itself by a known distance, halfway through the run.
///
/// Absolute positions are not comparable: the page's coordinates are
/// viewport-relative while `set_cursor_rectangle` is surface-relative, and
/// window decorations sit between the two.  A *displacement* cancels that
/// constant out, and at a 2x surface scale it also pins the logical→physical
/// conversion, which is the part of the forwarding that can be wrong while
/// still looking plausible.
const CARET_PAGE: &str = r#"<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>waiting</title>
<style>
  html, body { margin: 0; padding: 0; }
  #f { position: fixed; left: 120px; top: 80px; width: 200px; height: 30px;
       font-size: 16px; box-sizing: border-box; }
</style>
<input id="f" autofocus>
<script>
  const f = document.getElementById('f');
  f.focus();
  function report() {
    const r = f.getBoundingClientRect();
    document.title =
      `RECT:${Math.round(r.left)},${Math.round(r.top)},` +
      `${Math.round(r.width)},${Math.round(r.height)},` +
      `${devicePixelRatio}`;
  }
  report();
  addEventListener('resize', report);
  f.addEventListener('focus', report);
  setTimeout(() => {
    f.style.left = '220px';
    f.style.top = '180px';
    f.focus();
    report();
  }, 15000);
</script>
"#;

/// How far the page moves its field, in CSS pixels.
const FIELD_MOVE: (i32, i32) = (100, 100);

/// Kills the browser even when an assertion unwinds past it.
struct Browser(Child);

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Write `html` into a scratch dir and open it in a Chromium that speaks
/// text-input-v3 to `wayland_display`.
fn spawn_chromium(dir: &std::path::Path, html: &str, wayland_display: &str) -> Browser {
    spawn_chromium_window(dir, html, wayland_display, true)
}

fn spawn_chromium_window(
    dir: &std::path::Path,
    html: &str,
    wayland_display: &str,
    app_mode: bool,
) -> Browser {
    std::fs::create_dir_all(dir).expect("scratch dir");
    let page = dir.join("page.html");
    std::fs::File::create(&page)
        .and_then(|mut f| f.write_all(html.as_bytes()))
        .expect("write page");
    Browser(
        Command::new(chromium_binary())
            .args([
                "--ozone-platform=wayland",
                // Without this Chromium never binds zwp_text_input_v3 at all
                // and composed text has nowhere to go.
                "--enable-wayland-ime",
                "--wayland-text-input-version=3",
                "--no-sandbox",
                "--disable-gpu",
                "--no-first-run",
                "--noerrdialogs",
                "--disable-features=Translate",
            ])
            .arg(format!("--user-data-dir={}", dir.join("profile").display()))
            .arg(format!(
                "{}file://{}",
                if app_mode { "--app=" } else { "" },
                page.display(),
            ))
            .env("WAYLAND_DISPLAY", wayland_display)
            .env("GDK_BACKEND", "wayland")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn chromium"),
    )
}

fn chromium_binary() -> std::ffi::OsString {
    std::env::var_os("YAS_TEST_CHROMIUM").unwrap_or_else(|| "chromium".into())
}

#[test]
#[ignore = "starts a real browser"]
fn chromium_honors_fractional_zoom_screen_bounds() {
    let dir = std::env::temp_dir().join(format!("yas-fractional-screen-{}", std::process::id()));
    let handle = spawn_compositor(false, Arc::new(|| {}), "");
    let browser = spawn_chromium_window(
        &dir,
        r#"<!doctype html><title>waiting</title><p>Fractional zoom</p>
        <script>
        function report() {
          document.title = `SCREEN:${screen.width},${screen.height},${outerWidth}`;
        }
        addEventListener('resize', report);
        setInterval(report, 100);
        report();
        </script>"#,
        &handle.socket_name,
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut surface_id = None;
    let mut last_frame = Instant::now();
    let mut last_title = String::new();
    let mut filled = false;
    while Instant::now() < deadline && !filled {
        if let Some(surface_id) = surface_id
            && last_frame.elapsed() >= Duration::from_millis(50)
        {
            handle
                .command_tx
                .send(CompositorCommand::RequestFrame {
                    surface_id,
                    presentation_at: Instant::now(),
                })
                .unwrap();
            handle.wake();
            last_frame = Instant::now();
        }
        match handle.event_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(CompositorEvent::SurfaceCreated { surface_id: id, .. }) => {
                surface_id = Some(id);
                handle
                    .command_tx
                    .send(CompositorCommand::SurfaceResize {
                        surface_id: id,
                        width: 1600,
                        height: 1200,
                        // 2x iPad display, relative zoom 80%.
                        scale_120: 192,
                    })
                    .unwrap();
                handle.wake();
            }
            Ok(CompositorEvent::SurfaceTitle { title, .. }) => {
                filled =
                    title == "SCREEN:1000,750,1000" || title.starts_with("SCREEN:1000,750,1000 - ");
                last_title = title;
            }
            _ => {}
        }
    }
    drop(browser);
    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        filled,
        "Chromium must see the full logical screen and fill its width at 80% zoom; last title: {last_title}",
    );
}

#[test]
#[ignore = "starts a real browser"]
fn chromium_honors_retina_browser_zoom_scale() {
    let dir = std::env::temp_dir().join(format!("yas-high-dpi-{}", std::process::id()));
    let handle = spawn_compositor(false, Arc::new(|| {}), "");
    let browser = spawn_chromium_window(
        &dir,
        r#"<!doctype html><title>waiting</title><p>High DPI text</p>
        <script>
        function report() {
          document.title = `DENSITY:${devicePixelRatio}`;
        }
        addEventListener('resize', report);
        setInterval(report, 100);
        report();
        </script>"#,
        &handle.socket_name,
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut high_density = false;
    let mut geometry = None;
    let mut surface_id = None;
    let mut resized = false;
    let mut last_frame = Instant::now();
    while Instant::now() < deadline {
        if high_density && geometry.is_some() {
            break;
        }
        // An active viewer normally paces the browser's frame callbacks.
        if resized && last_frame.elapsed() >= Duration::from_millis(50) {
            handle
                .command_tx
                .send(CompositorCommand::RequestFrame {
                    surface_id: surface_id.unwrap(),
                    presentation_at: Instant::now(),
                })
                .unwrap();
            handle.wake();
            last_frame = Instant::now();
        }
        match handle.event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(CompositorEvent::SurfaceCreated { surface_id: id, .. }) => surface_id = Some(id),
            Err(_) if surface_id.is_some() && !resized => {
                resized = true;
                handle
                    .command_tx
                    .send(CompositorCommand::SurfaceResize {
                        surface_id: surface_id.unwrap(),
                        width: 1200,
                        height: 1000,
                        // Retina 2x multiplied by browser zoom 4x.
                        scale_120: 960,
                    })
                    .unwrap();
                handle.wake();
            }
            Ok(CompositorEvent::SurfaceTitle { title, .. }) => {
                high_density = title == "DENSITY:8" || title.starts_with("DENSITY:8 - ");
            }
            Ok(CompositorEvent::SurfaceResized {
                width,
                height,
                logical_width,
                logical_height,
                ..
            }) if resized
                && width == logical_width * 8
                && height == logical_height * 8
                && (151..1000).contains(&logical_width) =>
            {
                geometry = Some((width, height, logical_width, logical_height));
            }
            _ => {}
        }
    }
    drop(browser);
    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        high_density,
        "Chromium must observe an 8x device pixel ratio"
    );
    let (width, height, logical_width, logical_height) =
        geometry.expect("Chromium must render its minimum window size at 8x density");
    eprintln!(
        "Chromium rendered {logical_width}x{logical_height} at 8x as {width}x{height}; requested 1200x1000"
    );
}

#[test]
#[ignore = "starts a real browser"]
fn chromium_reports_its_initial_caret_before_composition() {
    if Command::new(chromium_binary())
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("chromium not on PATH; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("yas-ime-initial-{}", std::process::id()));
    let handle = spawn_compositor(false, Arc::new(|| {}), "");
    let browser = spawn_chromium(&dir, CARET_PAGE, &handle.socket_name);
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut caret = None;
    while Instant::now() < deadline {
        match handle.event_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(CompositorEvent::SurfaceCreated { surface_id, .. }) => {
                handle
                    .command_tx
                    .send(CompositorCommand::SurfaceFocus { surface_id })
                    .unwrap();
                handle.wake();
            }
            Ok(CompositorEvent::SurfaceTextInput {
                enabled: true,
                cursor_rect: Some(rect),
                ..
            }) => {
                caret = Some(rect);
                break;
            }
            _ => {}
        }
    }
    drop(browser);
    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
    let (x, y, _, height) =
        caret.expect("the focused field must report its caret before any preedit or key");
    assert!(
        x >= 120 && y >= 80 && height > 0,
        "unexpected initial caret: {caret:?}"
    );
}

#[test]
#[ignore = "starts a real browser"]
fn chromium_inserts_composed_text() {
    if Command::new(chromium_binary())
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("chromium not on PATH; skipping");
        return;
    }

    let dir = std::env::temp_dir().join(format!("yas-ime-{}", std::process::id()));
    let handle = spawn_compositor(false, Arc::new(|| {}), "");
    let _browser = spawn_chromium(&dir, PAGE, &handle.socket_name);

    // Chromium takes its time to come up, map a window, and lay out the page.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut surface_id = None;
    let mut inserted = String::new();
    let mut preedit = String::new();
    let mut typed = false;

    while Instant::now() < deadline {
        let Ok(ev) = handle.event_rx.recv_timeout(Duration::from_millis(250)) else {
            // A quiet moment after the window exists is the page having
            // settled: focus it, compose, then commit — the order a user
            // types in, so the commit has a preedit to replace.
            if let Some(id) = surface_id
                && !typed
            {
                handle
                    .command_tx
                    .send(CompositorCommand::SurfaceFocus { surface_id: id })
                    .expect("focus");
                std::thread::sleep(Duration::from_millis(500));
                handle
                    .command_tx
                    .send(CompositorCommand::Preedit {
                        text: COMPOSING.to_string(),
                        cursor: COMPOSING.len() as u16,
                    })
                    .expect("compose");
                std::thread::sleep(Duration::from_millis(300));
                handle
                    .command_tx
                    .send(CompositorCommand::TextInput {
                        text: TYPED.to_string(),
                    })
                    .expect("type");
                typed = true;
            }
            continue;
        };
        match ev {
            CompositorEvent::SurfaceCreated { surface_id: id, .. } => surface_id = Some(id),
            CompositorEvent::SurfaceTitle { title: t, .. } => {
                // With --nocapture this is the whole story in two lines:
                // what the app drew while composing, and what it kept.
                eprintln!("[title] {t}");
                if let Some(got) = t.strip_prefix("PRE:") {
                    preedit = got.to_string();
                } else if let Some(got) = t.strip_prefix("GOT:") {
                    inserted = got.to_string();
                    // Chromium retitles per keystroke; the last one is the
                    // whole field, so keep reading until it stops changing.
                    if inserted == TYPED {
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        !inserted.is_empty(),
        "chromium never reported any text -- the field was never focused, \
         so this run says nothing about the input method"
    );
    assert_eq!(
        inserted, TYPED,
        "chromium should have inserted the composed characters too"
    );
    assert_eq!(
        preedit, COMPOSING,
        "chromium should have shown the composition while it was pending"
    );
}

/// The caret rectangle a real toolkit sends, in the space the browser client
/// draws in.
///
/// `set_cursor_rectangle` is surface-local and *logical*; the frame the web
/// client lays its canvas out in is physical.  At scale 1 the two agree and a
/// missing conversion would pass unnoticed, so this runs the surface at 2x
/// and asks the page where its field is in (scale-independent) CSS pixels.
/// The forwarded rectangle has to land inside twice that box.
#[test]
#[ignore = "starts a real browser"]
fn chromium_reports_where_its_caret_is() {
    if Command::new(chromium_binary())
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("chromium not on PATH; skipping");
        return;
    }

    let dir = std::env::temp_dir().join(format!("yas-ime-caret-{}", std::process::id()));
    let handle = spawn_compositor(false, Arc::new(|| {}), "");
    let _browser = spawn_chromium(&dir, CARET_PAGE, &handle.socket_name);

    const SCALE_120: u16 = 240;
    let scale = i32::from(SCALE_120) / 120;
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut surface_id = None;
    // Field position, as the page reports it, and the caret Chromium sent
    // while composing there. One pair before the page moves its field, one
    // after.
    /// Where the page put its field, and the caret we were handed there.
    type Measurement = ((i32, i32), (i32, i32, i32, i32));
    let mut field: Option<(i32, i32)> = None;
    let mut before: Option<Measurement> = None;
    let mut after: Option<Measurement> = None;
    let mut driven = false;
    let mut composing = false;
    let mut latest_caret = None;

    /// Keep a composition active while measuring the caret after movement.
    fn compose(handle: &yas_compositor::CompositorHandle) {
        handle
            .command_tx
            .send(CompositorCommand::Preedit {
                text: COMPOSING.to_string(),
                cursor: COMPOSING.len() as u16,
            })
            .expect("compose");
    }

    while Instant::now() < deadline {
        let Ok(ev) = handle.event_rx.recv_timeout(Duration::from_millis(250)) else {
            // A quiet moment after the window exists: put it at a known size
            // and scale, hand it the keyboard so its field enables its text
            // input. Wait for the page to observe the scale before composing.
            if let Some(id) = surface_id
                && !driven
            {
                handle
                    .command_tx
                    .send(CompositorCommand::SurfaceResize {
                        surface_id: id,
                        width: 800,
                        height: 600,
                        scale_120: SCALE_120,
                    })
                    .expect("resize");
                std::thread::sleep(Duration::from_millis(1500));
                handle
                    .command_tx
                    .send(CompositorCommand::SurfaceFocus { surface_id: id })
                    .expect("focus");
                driven = true;
            }
            continue;
        };
        match ev {
            CompositorEvent::SurfaceCreated { surface_id: id, .. } => surface_id = Some(id),
            CompositorEvent::SurfaceTitle { title, .. } => {
                eprintln!("[title] {title}");
                if let Some(rest) = title.strip_prefix("RECT:") {
                    let n: Vec<i32> = rest
                        .split(',')
                        .filter_map(|v| v.parse::<f64>().ok())
                        .map(|v| v as i32)
                        .collect();
                    if n.len() >= 5 {
                        field = Some((n[0], n[1]));
                        if n[4] == scale && !composing {
                            compose(&handle);
                            composing = true;
                        }
                    }
                }
            }
            CompositorEvent::SurfaceTextInput {
                cursor_rect: Some(rect),
                ..
            } => {
                if !composing {
                    continue;
                }
                eprintln!("[caret] {rect:?} field={field:?}");
                let Some(at) = field else { continue };
                latest_caret = Some(rect);
                if before.is_none() {
                    before = Some((at, rect));
                }
            }
            _ => {}
        }
        // Text-input state and window titles travel independently. The moved
        // caret may arrive before the title announcing the field's new box.
        if let (Some((was, first)), Some(at), Some(rect)) = (before, field, latest_caret)
            && was != at
            && first != rect
        {
            after = Some((at, rect));
            break;
        }
    }

    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);

    let ((ax, ay), (cx, cy, _, ch)) = before.expect(
        "chromium never sent a cursor rectangle -- with none forwarded the \
         browser has nowhere to park its IME capture element",
    );
    let ((bx, by), (dx, dy, ..)) = after.expect(
        "the field never moved under a second composition, so nothing here \
         pins the coordinate conversion",
    );

    assert_eq!(
        (bx - ax, by - ay),
        FIELD_MOVE,
        "the page should have moved its field by exactly this much"
    );
    // The caret follows in physical pixels: the same move, times the scale.
    assert_eq!(
        (dx - cx, dy - cy),
        (FIELD_MOVE.0 * scale, FIELD_MOVE.1 * scale),
        "a field that moved {FIELD_MOVE:?} CSS px at {scale}x should move the \
         forwarded caret by {scale}x that; got ({cx},{cy}) then ({dx},{dy})"
    );
    // A caret is a text line tall, in physical pixels: a rectangle still in
    // logical units would come back at half this.
    assert!(
        (24..=80).contains(&ch),
        "caret height {ch} does not look like a 16px line at {scale}x"
    );
}
