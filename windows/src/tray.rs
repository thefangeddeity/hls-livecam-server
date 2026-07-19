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

pub enum TrayAction {
    Show,
    Quit,
}

/// Non-blocking poll -- called once per GUI frame.
pub fn poll(tray: &Tray) -> Option<TrayAction> {
    let event = MenuEvent::receiver().try_recv().ok()?;
    if event.id == tray.show_id {
        Some(TrayAction::Show)
    } else if event.id == tray.quit_id {
        Some(TrayAction::Quit)
    } else {
        None
    }
}
