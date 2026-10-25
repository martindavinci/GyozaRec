//! The notification-area icon and its menu.

use gyozarec::i18n::Texts;
use gyozarec::quick::QuickAction;
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Menu id prefix: `tray:<quick action name | show | options | exit>`.
pub const MENU_PREFIX: &str = "tray:";
pub const SHOW: &str = "tray:show";
pub const OPTIONS: &str = "tray:options";
pub const EXIT: &str = "tray:exit";
pub const CONVERT: &str = "tray:convert";

/// What a tray menu id asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Quick(QuickAction),
    Convert,
    Show,
    Options,
    Exit,
}

pub fn command(menu_id: &str) -> Option<TrayCommand> {
    match menu_id {
        SHOW => Some(TrayCommand::Show),
        OPTIONS => Some(TrayCommand::Options),
        EXIT => Some(TrayCommand::Exit),
        CONVERT => Some(TrayCommand::Convert),
        other => {
            let name = other.strip_prefix(MENU_PREFIX)?;
            QuickAction::from_name(name).map(TrayCommand::Quick)
        }
    }
}

fn icon() -> Option<Icon> {
    let png = include_bytes!("../../assets/icon/gyozarec-256.png");
    let img = image::load_from_memory(png).ok()?;
    let small = img.resize_exact(32, 32, image::imageops::FilterType::Triangle).to_rgba8();
    Icon::from_rgba(small.into_raw(), 32, 32).ok()
}

/// `None` if Windows refuses the icon; the caller then treats the tray as off.
pub fn create(texts: &Texts) -> Option<TrayIcon> {
    let quick = |action: QuickAction, label: &str| {
        MenuItem::with_id(format!("{MENU_PREFIX}{}", action.name()), label, true, None)
    };
    let menu = Menu::new();
    let items = [
        quick(QuickAction::Current, texts.capture),
        quick(QuickAction::Image, texts.quick_actions[0]),
        quick(QuickAction::ImageFullScreen, texts.quick_actions[1]),
        quick(QuickAction::Record, texts.quick_actions[2]),
        quick(QuickAction::RecordFullScreen, texts.quick_actions[3]),
    ];
    for item in &items {
        menu.append(item).ok()?;
    }
    menu.append(&MenuItem::with_id(CONVERT, texts.convert_video, true, None)).ok()?;
    menu.append(&PredefinedMenuItem::separator()).ok()?;
    menu.append(&MenuItem::with_id(SHOW, texts.tray_show, true, None)).ok()?;
    menu.append(&MenuItem::with_id(OPTIONS, texts.options, true, None)).ok()?;
    menu.append(&PredefinedMenuItem::separator()).ok()?;
    menu.append(&MenuItem::with_id(EXIT, texts.tray_exit, true, None)).ok()?;
    TrayIconBuilder::new()
        .with_tooltip("GyozaRec")
        .with_icon(icon()?)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_menu_ids_map_to_commands() {
        for action in QuickAction::ALL {
            let id = format!("{MENU_PREFIX}{}", action.name());
            assert_eq!(command(&id), Some(TrayCommand::Quick(action)));
        }
        assert_eq!(command(SHOW), Some(TrayCommand::Show));
        assert_eq!(command(OPTIONS), Some(TrayCommand::Options));
        assert_eq!(command(EXIT), Some(TrayCommand::Exit));
        assert_eq!(command(CONVERT), Some(TrayCommand::Convert));
        assert_eq!(command("tray:nope"), None);
        assert_eq!(command("area:1"), None);
    }
}
