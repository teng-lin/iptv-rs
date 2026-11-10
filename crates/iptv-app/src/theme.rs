//! PLACEHOLDER THEME: colour values and state rules taken from the TiviMate
//! 5.3.3 visual-system reference (`~/src/tivimate/docs/04-visual-system.md`).
//! The accents are the public Material Design 500/200 swatches. Replace this
//! module with the app's own design before any public release (tracked in
//! README "Before release").
//!
//! Only colour values and state rules live here: no artwork, icons or font
//! files are copied from the APK (the font is the system's Roboto).

/// An accent bundle: lighter tone and main accent (`0xRRGGBB`). The focus
/// fill is the main accent at alpha `0x60`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accent {
    pub name: &'static str,
    pub light: u32,
    pub main: u32,
}

/// A background bundle: panel, window and alternate (elevated) surfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Background {
    pub name: &'static str,
    pub panel: u32,
    pub window: u32,
    pub alt: u32,
}

const fn a(name: &'static str, light: u32, main: u32) -> Accent {
    Accent { name, light, main }
}
const fn b(name: &'static str, panel: u32, window: u32, alt: u32) -> Background {
    Background {
        name,
        panel,
        window,
        alt,
    }
}

pub const ACCENTS: [Accent; 16] = [
    a("Amber", 0xFFE082, 0xFFC107),
    a("Black", 0x000000, 0x000000),
    a("Blue", 0x90CAF9, 0x2196F3),
    a("Blue grey", 0xB0BEC5, 0x607D8B),
    a("Brown", 0xBCAAA4, 0x795548),
    a("Cyan", 0x80DEEA, 0x00BCD4),
    a("Green", 0xA5D6A7, 0x4CAF50),
    a("Grey", 0xEEEEEE, 0x9E9E9E),
    a("Indigo", 0x9FA8DA, 0x3F51B5),
    a("Lime", 0xE6EE9C, 0xCDDC39),
    a("Orange", 0xFFCC80, 0xFF9800),
    a("Pink", 0xF48FB1, 0xE91E63),
    a("Purple", 0xCE93D8, 0x9C27B0),
    a("Red", 0xEF9A9A, 0xF44336),
    a("Teal", 0x80CBC4, 0x009688),
    a("Yellow", 0xFFF59D, 0xFFEB3B),
];

pub const BACKGROUNDS: [Background; 18] = [
    b("Amber", 0x58491D, 0x3A3113, 0x756226),
    b("Black", 0x131313, 0x060606, 0x202020),
    b("Blue", 0x1F415C, 0x142B3D, 0x29577B),
    b("Blue grey", 0x2C333C, 0x1C232C, 0x3C434C),
    b("Brown", 0x362923, 0x201913, 0x4C3933),
    b("Cyan", 0x174247, 0x0F2C2F, 0x1F585F),
    b("Dark", 0x232629, 0x131619, 0x333639),
    b("Dark grey", 0x262626, 0x161616, 0x363636),
    b("Green", 0x194C1B, 0x103312, 0x216624),
    b("Grey", 0x363636, 0x262626, 0x464646),
    b("Indigo", 0x1F295F, 0x151B3F, 0x2A377F),
    b("Lime", 0x575C1F, 0x3A3D14, 0x747B29),
    b("Orange", 0x553E1C, 0x392912, 0x725325),
    b("Pink", 0x642138, 0x431625, 0x862D4A),
    b("Purple", 0x4E1D57, 0x34133A, 0x692674),
    b("Red", 0x6B2523, 0x471817, 0x8F312F),
    b("Teal", 0x164440, 0x0E2D2A, 0x1D5B55),
    b("Yellow", 0x696223, 0x464117, 0x8D822F),
];

/// Alpha of the focus/selection fill over the accent (~37.6%).
pub const FOCUS_ALPHA: u8 = 0x60;
/// Alpha of a subtle resting button (`#16FFFFFF`, ~8.6%).
pub const RESTING_ALPHA: u8 = 0x16;
/// Text: primary white; secondary and disabled are white at these alphas.
pub const TEXT_SECONDARY_ALPHA: u8 = 0xA0;
pub const TEXT_DISABLED_ALPHA: u8 = 0x60;
/// Corner radii: focus shape and larger rounded surfaces, in dp.
pub const RADIUS: f32 = 6.0;
pub const RADIUS_LARGE: f32 = 16.0;
/// Base text size (sp) and list padding (dp).
pub const TEXT_SIZE: f32 = 14.0;
pub const LIST_PADDING: f32 = 16.0;

/// The theme in use: one accent and one background bundle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub accent: Accent,
    pub background: Background,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            accent: ACCENTS[2],
            background: BACKGROUNDS[6],
        } // Blue on Dark
    }
}

/// `0xRRGGBB` with alpha `a` as `0xRRGGBBAA` (NSH's `rgba`).
pub const fn with_alpha(rgb: u32, a: u8) -> u32 {
    (rgb << 8) | a as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_packs_after_rgb() {
        assert_eq!(with_alpha(0x2196F3, FOCUS_ALPHA), 0x2196F360);
    }

    #[test]
    fn bundles_are_complete_and_unique() {
        let mut names: Vec<_> = ACCENTS.iter().map(|a| a.name).collect();
        names.dedup();
        assert_eq!(names.len(), 16);
        assert_eq!(BACKGROUNDS.len(), 18);
    }
}
