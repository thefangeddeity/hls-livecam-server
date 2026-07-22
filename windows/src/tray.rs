//! System tray icon -- minimize-to-tray, "nice-to-have" per the brief.
//!
//! Must be built on the GUI/main thread (Windows tray APIs are thread-
//! affine the way window handles are), so this is constructed inside the
//! eframe::run_native creation closure, not earlier in main(). If this
//! ever fails to build on a given machine, the window itself still has
//! normal minimize/close controls -- a tray icon is additive, not load-
//! bearing, per the brief's explicit fallback permission.

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

pub struct Tray {
    pub icon: TrayIcon,
    pub show_id: tray_icon::menu::MenuId,
    pub quit_id: tray_icon::menu::MenuId,
}

pub fn build() -> Option<Tray> {
    let menu = Menu::new();
    let show_item = MenuItem::new("Show", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let show_id = show_item.id().clone();
    let quit_id = quit_item.id().clone();
    menu.append(&show_item).ok()?;
    menu.append(&quit_item).ok()?;

    let icon = TrayIconBuilder::new()
        .with_tooltip("hls-livecam-win")
        .with_icon(app_icon())
        .with_menu(Box::new(menu))
        .build()
        .ok()?;

    Some(Tray {
        icon,
        show_id,
        quit_id,
    })
}

/// A tiny solid green square -- matches the app's palette, no asset file
/// needed for a v1 tray glyph.
fn app_icon() -> tray_icon::Icon {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px[0] = 0x33;
        px[1] = 0xff;
        px[2] = 0x55;
        px[3] = 0xff;
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE).expect("valid icon buffer")
}

/// Handle tray menu clicks on a DEDICATED thread, not inside the GUI's
/// update() -- run-8 fix. The window-X policy hides the window to the tray
/// (Visible(false)), and a hidden window stops repainting, so update() no
/// longer runs to poll the menu channel. Result: the tray "Quit" click
/// landed in the channel but nobody read it -- the app couldn't be quit
/// from the tray at all once minimized (operator: "won't shut down even
/// from quit menu"). This thread blocks on the menu channel independently
/// of the GUI redraw state:
///
///   * Quit -> reap EVERY child (capture + mediamtx via pipeline.shutdown,
///     the preview tap via preview.shutdown) and then hard-exit. This is
///     the "leaves no trace" quit: no orphaned ffmpeg/mediamtx, no lingering
///     tray icon (it dies with the process). Independent of on_exit, which
///     an abrupt exit path wouldn't run.
///   * Show -> un-hide + focus the window and wake the loop (a background
///     request_repaint reaches the event loop even while it's parked
///     waiting, hidden).
pub fn spawn_menu_handler(
    tray: &Tray,
    ctx: eframe::egui::Context,
    pipeline: std::sync::Arc<crate::pipeline::Pipeline>,
    preview: crate::video_preview::PreviewCtl,
    rt: tokio::runtime::Handle,
) {
    let show_id = tray.show_id.clone();
    let quit_id = tray.quit_id.clone();
    std::thread::spawn(move || {
        let rx = MenuEvent::receiver();
        while let Ok(event) = rx.recv() {
            if event.id == quit_id {
                rt.block_on(async {
                    pipeline.shutdown().await;
                    preview.shutdown().await;
                });
                std::process::exit(0);
            } else if event.id == show_id {
                use eframe::egui::ViewportCommand;
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                ctx.request_repaint();
            }
        }
    });
}
