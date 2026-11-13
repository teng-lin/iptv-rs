//! Interface languages and translations. Pure, deterministic and platform independent.
//! English source messages are keys; untranslated messages retain their source text.

/// A supported interface language. Names stay in their own language so the
/// language picker remains usable after an accidental selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub tag: &'static str,
    pub name: &'static str,
}

/// Column order in the UTF-8 catalogue (English is the source column).
pub const LANGUAGES: &[Language] = &[
    Language {
        tag: "en",
        name: "English",
    },
    Language {
        tag: "es",
        name: "Español",
    },
    Language {
        tag: "fr",
        name: "Français",
    },
    Language {
        tag: "de",
        name: "Deutsch",
    },
    Language {
        tag: "it",
        name: "Italiano",
    },
    Language {
        tag: "pt",
        name: "Português",
    },
    Language {
        tag: "ru",
        name: "Русский",
    },
    Language {
        tag: "zh",
        name: "简体中文",
    },
    Language {
        tag: "zh-Hant",
        name: "繁體中文",
    },
    Language {
        tag: "ja",
        name: "日本語",
    },
    Language {
        tag: "ko",
        name: "한국어",
    },
];

/// Resolve regional/script tags, including POSIX locale names. Unsupported
/// languages fall back to English; Chinese regions select the right script.
pub fn resolve(preference: &str, system: Option<&str>) -> &'static Language {
    let tag = if preference == "system" {
        system.unwrap_or("en")
    } else {
        preference
    };
    let normalized = tag.trim().replace('_', "-").to_ascii_lowercase();
    let tag = normalized.split(['.', '@']).next().unwrap_or("");
    let parts: Vec<_> = tag.split('-').collect();
    let base = parts.first().copied().unwrap_or("");
    let language = if base == "zh" {
        if parts.contains(&"hant")
            || (!parts.contains(&"hans") && parts.iter().any(|p| matches!(*p, "tw" | "hk" | "mo")))
        {
            "zh-Hant"
        } else {
            "zh"
        }
    } else {
        base
    };
    LANGUAGES
        .iter()
        .find(|l| l.tag == language)
        .unwrap_or(&LANGUAGES[0])
}

const CATALOGUE: &str = include_str!("i18n/messages.tsv");

type Catalogue = std::collections::HashMap<&'static str, Vec<&'static str>>;
static MESSAGES: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();

/// Translate only application-owned messages, never playlist/channel/EPG text.
/// Unknown source text and unsupported languages safely retain English.
pub fn translate<'a>(language: &Language, source: &'a str) -> &'a str {
    let Some(column) = LANGUAGES.iter().position(|l| l.tag == language.tag) else {
        return source;
    };
    if column == 0 {
        return source;
    }
    let messages = MESSAGES.get_or_init(|| {
        CATALOGUE
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let columns: Vec<_> = line.split('\t').collect();
                (columns[0], columns)
            })
            .collect()
    });
    messages
        .get(source)
        .and_then(|row| row.get(column))
        .copied()
        .filter(|s| !s.is_empty())
        .unwrap_or(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regional_and_system_preferences_resolve() {
        for (tag, expected) in [
            ("es-MX", "es"),
            ("pt_BR.UTF-8", "pt"),
            ("ZH_tw", "zh-Hant"),
            ("zh-HK", "zh-Hant"),
            ("zh-Hans-TW", "zh"),
            ("zh-Hant-CN", "zh-Hant"),
            ("ja-JP", "ja"),
            ("xx", "en"),
            ("C", "en"),
        ] {
            assert_eq!(resolve(tag, None).tag, expected, "{tag}");
        }
        assert_eq!(resolve("system", Some("fr-CA")).tag, "fr");
        assert_eq!(resolve("de", Some("es")).tag, "de");
        assert_eq!(resolve("system", None).tag, "en");
    }

    #[test]
    fn catalogue_is_complete_unique_and_has_no_blank_translations() {
        let mut seen = std::collections::HashSet::new();
        for line in CATALOGUE.lines() {
            let columns: Vec<_> = line.split('\t').collect();
            assert_eq!(columns.len(), LANGUAGES.len(), "{line}");
            assert!(seen.insert(columns[0]), "duplicate: {}", columns[0]);
            assert!(columns.iter().all(|c| !c.trim().is_empty()), "{line}");
            for language in LANGUAGES {
                assert!(!translate(language, columns[0]).is_empty());
            }
        }
        assert!(seen.len() >= 100);
    }

    #[test]
    fn translates_and_preserves_unknown_messages() {
        assert_eq!(translate(resolve("es", None), "Settings"), "Ajustes");
        assert_eq!(translate(resolve("zh-TW", None), "Settings"), "設定");
        assert_eq!(translate(resolve("en", None), "Settings"), "Settings");
        assert_eq!(
            translate(resolve("fr", None), "Untranslated message"),
            "Untranslated message"
        );
    }
}
