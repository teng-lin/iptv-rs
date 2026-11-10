//! The bundled fonts (Google's Roboto and Roboto Condensed, SIL OFL 1.1;
//! `fonts/README.md`) and the type of each text role, in sp at full size
//! (desktop windows smaller than full screen scale them, `scale.rs`). They are
//! registered at startup on every platform, and Roboto becomes the app's
//! default family (the tokens' `font-sans`). Every family list ends in
//! `sans-serif`: the scripts the subsets leave out (Arabic, Hebrew, CJK,
//! Thai, Devanagari, ▶, ★, emoji) come from the system's fonts.

use nsh_core::prelude::*;

/// Roboto: channel numbers and names, the ruler, the header.
pub const SANS: &str = "Roboto, sans-serif";
/// Roboto Condensed: programme titles in the grid.
pub const CONDENSED: &str = "'Roboto Condensed', Roboto, sans-serif";
/// The header's title (Roboto Medium).
pub const MEDIUM: FontWeight = FontWeight::MEDIUM;

/// Programme titles in the grid's cells.
pub const CELL_TITLE: f32 = 20.0;
/// Channel numbers and names.
pub const CHANNEL: f32 = 20.0;
/// The ruler's date and slot times.
pub const RULER: f32 = 18.0;
/// The header: the focused programme's title, its times line, and its
/// episode line and description.
pub const HEADER_TITLE: f32 = 22.0;
pub const HEADER_TIMES: f32 = 17.0;
pub const HEADER_BODY: f32 = 16.0;

/// The lists' type (the groups columns, the player's channel list, search,
/// the favourites editor, the menu): the guide's, so a name reads the same
/// in a list as in the guide's channel column (rows: `list_rows.rs`).
/// A list item's name: the guide's channel names.
pub const LIST_ITEM: f32 = CHANNEL;
/// An item's second line (what is on, a result's channel and times): the
/// header's description.
pub const LIST_SECONDARY: f32 = HEADER_BODY;
/// A list's heading ("Groups", the list's name, the menu's headings): the
/// header's times line.
pub const SECTION: f32 = HEADER_TIMES;
/// Hints, status lines and badges.
pub const CAPTION: f32 = 15.0;
/// A screen's title (Search, the favourites editor): the header's title.
pub const TITLE: f32 = HEADER_TITLE;

/// The font files, WOFF2 (NSH unpacks them when it registers them).
pub const FILES: [(&str, &[u8]); 3] = [
    (
        "Roboto",
        include_bytes!("../fonts/Roboto-Regular.woff2").as_slice(),
    ),
    (
        "Roboto",
        include_bytes!("../fonts/Roboto-Medium.woff2").as_slice(),
    ),
    (
        "Roboto Condensed",
        include_bytes!("../fonts/RobotoCondensed-Regular.woff2").as_slice(),
    ),
];

/// Registers the fonts and makes Roboto the default family. Text laid out
/// before a font arrives is laid out again with it.
pub fn register() {
    for (family, data) in FILES {
        let answer = register_font(data);
        drop(spawn(async move {
            if let Err(e) = answer.await {
                log::warn!("iptv font {family} not registered: {e}");
            }
        }));
    }
    let mut tokens = (*tokens()).clone();
    tokens.set_font(nsh_core::element::FONT_SANS, SANS);
    set_tokens(tokens);
}

/// `name` cut to fit `width` with two dots ("Long chann.."), as measured by
/// `measure`; whole when it fits.
pub fn two_dots(name: &str, width: f32, measure: impl Fn(&str) -> f32) -> String {
    if width <= 0.0 || measure(name) <= width {
        return name.to_owned();
    }
    let ends: Vec<usize> = name.char_indices().map(|(i, _)| i).skip(1).collect();
    // The longest start that fits with the dots (binary search: widths grow
    // with the length).
    let (mut lo, mut hi) = (0, ends.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cut = format!("{}..", name[..ends[mid - 1]].trim_end());
        if measure(&cut) <= width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    match lo {
        0 => "..".to_owned(),
        n => format!("{}..", name[..ends[n - 1]].trim_end()),
    }
}

/// [`two_dots`] with the platform's text engine, in `size` sp Roboto.
pub fn fit(name: &str, width: f32, size: f32) -> String {
    let font = format!("{size}px Roboto");
    two_dots(name, width, |s| measure_text(&font, s).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_files_are_the_families_the_app_names() {
        for (family, data) in FILES {
            assert_eq!(nsh_core::font_family(data).as_deref(), Ok(family));
            assert!(nsh_core::font_data(data).unwrap().len() > data.len());
        }
        for list in [SANS, CONDENSED] {
            assert!(list.ends_with("sans-serif"), "{list}: system fallback");
        }
        assert!(CONDENSED.starts_with("'Roboto Condensed'"));
    }

    #[test]
    fn long_names_end_in_two_dots() {
        // Ten units a character.
        let measure = |s: &str| s.chars().count() as f32 * 10.0;
        assert_eq!(two_dots("Short", 100.0, measure), "Short");
        assert_eq!(two_dots("Exactly ten", 110.0, measure), "Exactly ten");
        assert_eq!(two_dots("A rather long name", 100.0, measure), "A rather..");
        assert_eq!(two_dots("Ελληνικό κανάλι", 80.0, measure), "Ελληνι..");
        assert_eq!(two_dots("Name", 25.0, measure), "..");
        assert_eq!(two_dots("Name", 0.0, measure), "Name");
    }
}
