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
    sys_locale::get_locale()
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
