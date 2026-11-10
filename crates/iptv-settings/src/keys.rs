//! Typed keys for the settings other crates read in code, so a read cannot
//! ask for the wrong type. The test suite checks each against the schema.
//! Add a constant here when a crate starts reading a setting.

/// A key whose value is a [`Value::Bool`](crate::Value::Bool).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoolKey(pub &'static str);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntKey(pub &'static str);
/// Milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurationKey(pub &'static str);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnumKey(pub &'static str);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextKey(pub &'static str);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColourKey(pub &'static str);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingsKey(pub &'static str);

// Input (iptv-nav).
pub const LONG_PRESS: DurationKey = DurationKey("remote.long_press");
pub const DIGIT_TIMEOUT: DurationKey = DurationKey("remote.digit_timeout");
pub const KEY_OVERRIDES: BindingsKey = BindingsKey("remote.key_overrides");

// Player panels (iptv-core panels::Timeouts).
pub const INFO_PANEL_TIMEOUT: DurationKey = DurationKey("player.info_panel_timeout");
pub const CONTROLS_PANEL_TIMEOUT: DurationKey = DurationKey("player.controls_panel_timeout");

// Channel switching.
pub const SWITCH_DELAY: DurationKey = DurationKey("guide.switch_delay");
pub const FULLSCREEN_TIMEOUT: DurationKey = DurationKey("guide.fullscreen_timeout");
pub const AUTOPLAY: BoolKey = BoolKey("guide.autoplay");
pub const BROWSE_MODE: EnumKey = EnumKey("guide.browse_mode");

// Appearance (iptv-app theme).
pub const ACCENT: ColourKey = ColourKey("appearance.accent");
pub const BACKGROUND: ColourKey = ColourKey("appearance.background");
pub const FONT_SIZE: EnumKey = EnumKey("appearance.font_size");

// Organisation.
pub const CHANNEL_SORT: EnumKey = EnumKey("channels.channel_sort");
pub const GROUP_SORT: EnumKey = EnumKey("channels.group_sort");
pub const RECENT_COUNT: IntKey = IntKey("history.recent_count");
pub const USER_AGENT: TextKey = TextKey("general.user_agent");

/// Every typed key with its expected type name, for the schema test.
pub const ALL: &[(&str, &str)] = &[
    (LONG_PRESS.0, "duration"),
    (DIGIT_TIMEOUT.0, "duration"),
    (KEY_OVERRIDES.0, "key_bindings"),
    (INFO_PANEL_TIMEOUT.0, "duration"),
    (CONTROLS_PANEL_TIMEOUT.0, "duration"),
    (SWITCH_DELAY.0, "duration"),
    (FULLSCREEN_TIMEOUT.0, "duration"),
    (AUTOPLAY.0, "bool"),
    (BROWSE_MODE.0, "enum"),
    (ACCENT.0, "colour"),
    (BACKGROUND.0, "colour"),
    (FONT_SIZE.0, "enum"),
    (CHANNEL_SORT.0, "enum"),
    (GROUP_SORT.0, "enum"),
    (RECENT_COUNT.0, "integer"),
    (USER_AGENT.0, "text"),
];
