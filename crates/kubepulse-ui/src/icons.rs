//! Turns host-defined icons and connection states into glyphs, so each one
//! looks the same everywhere it appears.

use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme as _, Icon, Sizable as _, spinner::Spinner},
    *,
};
use kubepulse_cluster::ConnectionState;
use kubepulse_extension_api::ViewIcon;

pub fn view_glyph(icon: ViewIcon) -> IconName {
    match icon {
        ViewIcon::Generic => IconName::Shapes,
        ViewIcon::Box => IconName::Box,
        ViewIcon::Layers => IconName::Layers,
        ViewIcon::Copy => IconName::Copy,
        ViewIcon::Database => IconName::Database,
        ViewIcon::Server => IconName::ServerCog,
        ViewIcon::Timer => IconName::Timer,
        ViewIcon::Schedule => IconName::CalendarClock,
        ViewIcon::Network => IconName::Network,
        ViewIcon::Globe => IconName::Globe,
        ViewIcon::Shield => IconName::ShieldCheck,
        ViewIcon::Waypoints => IconName::Waypoints,
        ViewIcon::SettingsFile => IconName::FileSliders,
        ViewIcon::Key => IconName::KeyRound,
        ViewIcon::Disk => IconName::HardDrive,
        ViewIcon::StorageClass => IconName::Layers2,
        ViewIcon::Account => IconName::UserCog,
        ViewIcon::Role => IconName::IdCard,
        ViewIcon::Link => IconName::Link,
    }
}

pub fn state_mark(state: &ConnectionState, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (name, color) = match state {
        ConnectionState::Connecting | ConnectionState::Authenticating => {
            return Spinner::new().xsmall().into_any_element();
        }
        ConnectionState::Connected => (IconName::CircleCheck, theme.success),
        ConnectionState::Disconnected => (IconName::Unplug, theme.muted_foreground),
        ConnectionState::Degraded { .. } => (IconName::TriangleAlert, theme.warning),
        ConnectionState::Failed { .. } => (IconName::CircleX, theme.danger),
    };
    Icon::new(name)
        .xsmall()
        .text_color(color)
        .into_any_element()
}
