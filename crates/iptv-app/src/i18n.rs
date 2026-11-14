//! Platform locale discovery lives in the shell; lookup lives in the core.
use iptv_core::i18n::{self, Language};
use iptv_settings::{Target, keys};

pub fn language() -> &'static Language {
    crate::settings::observe();
    let settings = crate::settings::get();
    let preference = settings.text(keys::LANGUAGE, &Target::Global);
    static SYSTEM: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let system = SYSTEM.get_or_init(system_locale);
    i18n::resolve(&preference, system.as_deref())
}

fn system_locale() -> Option<String> {
    // Headless scenarios pin a locale independently of the build machine.
    #[cfg(any(test, feature = "test-support"))]
    if let Ok(locale) = std::env::var("IPTV_RS_TEST_LOCALE") {
        return Some(locale);
    }
    #[cfg(test)]
    {
        Some("en".to_owned())
    }
    #[cfg(not(test))]
    {
        sys_locale::get_locale()
    }
}

/// Only call for application-owned messages, never provider content.
pub fn tr(source: &str) -> &str {
    i18n::translate(language(), source)
}

pub fn language_name(tag: &str) -> &'static str {
    if tag == "system" {
        return tr("System language");
    }
    i18n::LANGUAGES
        .iter()
        .find(|l| l.tag == tag)
        .map_or("", |l| l.name)
}

#[cfg(test)]
mod tests {
    /// The names of the controls that show an icon only (`.label(…)`, read
    /// by VoiceOver and TalkBack): each has a row in the catalogue, so a
    /// screen reader speaks the interface language. A name that only
    /// reached `tr` without a row would stay English without a sign.
    #[test]
    fn accessibility_labels_are_in_the_catalogue() {
        use iptv_core::i18n::{LANGUAGES, resolve, translate};
        const LABELS: [&str; 29] = [
            // The play bar.
            "Mute",
            "Volume",
            "Rewind",
            "Play or pause",
            "Fast-forward",
            "Favourite",
            "Subtitles and tracks",
            "Programme details",
            "Picture size",
            "Channel list",
            "Multiview",
            "Back to multiview",
            "Picture in picture",
            "TV guide",
            "Full screen",
            "Guide",
            // The video, the choosers and the guide's way back.
            "Video",
            "Close the chooser",
            "Groups",
            "Menu",
            // Multiview's toolbar and tiles.
            "Add screen",
            "Exit multiview",
            "Reduce screen",
            "Enlarge screen",
            "Remove screen",
            // The picture-in-picture window.
            "Back to the app",
            "Close",
            "Play",
            "Pause",
        ];
        for label in LABELS {
            // Some languages spell a word as English does ("Menu"), but
            // none of these is the same in every language.
            let translated = LANGUAGES
                .iter()
                .filter(|language| translate(language, label) != label)
                .count();
            assert!(translated > 0, "no catalogue row for {label:?}");
        }
        assert_eq!(translate(resolve("es", None), "Volume"), "Volumen");
        assert_eq!(translate(resolve("de", None), "Mute"), "Stumm");
        assert_eq!(translate(resolve("zh-TW", None), "Menu"), "選單");
    }

    #[test]
    fn picker_and_catalogue_languages_match() {
        let choices = iptv_settings::catalogue::choices::LANGUAGE.variants;
        assert_eq!(choices[0], "system");
        assert_eq!(
            &choices[1..],
            iptv_core::i18n::LANGUAGES
                .iter()
                .map(|l| l.tag)
                .collect::<Vec<_>>()
        );
    }
}
