//! Integrity of the schema and catalogue.

use std::collections::{BTreeMap, BTreeSet};

use iptv_settings::catalogue::{self, choices};
use iptv_settings::schema::{Effect, Test, ValueType};
use iptv_settings::validate::{check, is_token};
use iptv_settings::{Scope, Section};

fn all() -> Vec<&'static iptv_settings::Setting> {
    catalogue::settings().collect()
}

#[test]
fn keys_are_unique_across_settings_and_commands() {
    let mut seen = BTreeSet::new();
    for key in all()
        .iter()
        .map(|s| s.key)
        .chain(catalogue::commands().map(|c| c.key))
    {
        assert!(seen.insert(key), "duplicate key {key}");
    }
}

#[test]
fn keys_are_dotted_tokens_under_a_section() {
    for key in all()
        .iter()
        .map(|s| s.key)
        .chain(catalogue::commands().map(|c| c.key))
    {
        let parts: Vec<&str> = key.split('.').collect();
        assert!(parts.len() >= 2, "{key}: needs section.name");
        assert!(
            parts.iter().all(|p| is_token(p)),
            "{key}: segments must be tokens"
        );
        assert!(
            Section::from_key(parts[0]).is_some(),
            "{key}: unknown section prefix"
        );
    }
}

#[test]
fn settings_sit_in_their_sections_table() {
    for section in Section::ALL {
        for s in catalogue::section(section) {
            assert_eq!(s.section, section, "{}", s.key);
        }
    }
}

#[test]
fn defaults_are_valid_for_their_type() {
    for s in all() {
        assert_eq!(check(s, &s.default_value()), Ok(()), "{}", s.key);
    }
}

#[test]
fn numeric_ranges_are_well_formed() {
    for s in all() {
        let (min, max, step) = match s.ty {
            ValueType::Int { min, max, step, .. } => (min, max, step),
            ValueType::Duration {
                min_ms,
                max_ms,
                step_ms,
                ..
            } => (min_ms, max_ms, step_ms),
            _ => continue,
        };
        assert!(step > 0, "{}: step", s.key);
        assert!(min < max, "{}: range", s.key);
        assert_eq!(
            (max - min) % step,
            0,
            "{}: max must be on the step grid",
            s.key
        );
    }
}

#[test]
fn enum_and_colour_defaults_are_members() {
    for s in all() {
        let (choices, default) = match s.ty {
            ValueType::Enum { choices, default } => (choices, default),
            ValueType::Colour { palette, default } => (palette, default),
            _ => continue,
        };
        assert!(choices.contains(default), "{}: {default}", s.key);
    }
}

#[test]
fn choice_families_have_unique_token_variants() {
    for family in choices::ALL {
        assert!(is_token(family.family), "{}", family.family);
        let unique: BTreeSet<_> = family.variants.iter().collect();
        assert_eq!(unique.len(), family.variants.len(), "{}", family.family);
        assert!(
            family.variants.iter().all(|v| is_token(v)),
            "{}",
            family.family
        );
    }
    // Every family a setting uses is registered (so label extraction sees it).
    for s in all() {
        if let Some(c) = s.ty.choices() {
            assert!(
                choices::ALL.iter().any(|f| std::ptr::eq(*f, c)),
                "{}",
                s.key
            );
        }
    }
}

#[test]
fn dependencies_refer_to_existing_settings_of_a_matching_type() {
    for s in all() {
        for dep in s.depends {
            assert!(!dep.any_of.is_empty(), "{}: empty dependency", s.key);
            for w in dep.any_of {
                assert_ne!(w.key, s.key, "{}: depends on itself", s.key);
                let target = catalogue::setting(w.key)
                    .unwrap_or_else(|| panic!("{} depends on unknown {}", s.key, w.key));
                let ok = match (w.test, &target.ty) {
                    (Test::IsTrue | Test::IsFalse, ValueType::Bool { .. }) => true,
                    (Test::EnumIn(vs), ty) => {
                        let c = ty.choices().expect("EnumIn needs an enum or colour");
                        !vs.is_empty() && vs.iter().all(|v| c.contains(v))
                    }
                    (Test::NotEmpty, ValueType::Text { .. }) => true,
                    (Test::NonZero, ValueType::Int { .. } | ValueType::Duration { .. }) => true,
                    _ => false,
                };
                assert!(ok, "{} -> {}: test does not fit the type", s.key, w.key);
            }
        }
    }
}

#[test]
fn dependencies_are_acyclic() {
    fn visit(key: &'static str, path: &mut Vec<&'static str>) {
        assert!(!path.contains(&key), "dependency cycle: {path:?} -> {key}");
        path.push(key);
        let s = catalogue::setting(key).unwrap();
        for w in s.depends.iter().flat_map(|d| d.any_of) {
            visit(w.key, path);
        }
        path.pop();
    }
    for s in all() {
        visit(s.key, &mut Vec::new());
    }
}

#[test]
fn subsections_are_declared_and_used() {
    let mut used: BTreeMap<(Section, &str), usize> = BTreeMap::new();
    for s in all() {
        if let Some(sub) = s.subsection {
            assert!(
                s.section.subsections().contains(&sub),
                "{}: subsection {sub}",
                s.key
            );
            *used.entry((s.section, sub)).or_default() += 1;
        }
    }
    for c in catalogue::commands() {
        if let Some(sub) = c.subsection {
            assert!(
                c.section.subsections().contains(&sub),
                "{}: subsection {sub}",
                c.key
            );
            *used.entry((c.section, sub)).or_default() += 1;
        }
    }
    for section in Section::ALL {
        for sub in section.subsections() {
            assert!(
                used.contains_key(&(section, *sub)),
                "{}.{sub} is empty",
                section.key()
            );
        }
    }
}

#[test]
fn label_keys_are_unique() {
    let mut labels = BTreeSet::new();
    let mut add = |l: String| assert!(labels.insert(l.clone()), "duplicate label key {l}");
    for section in Section::ALL {
        add(section.label_key());
        for sub in section.subsections() {
            add(section.subsection_label_key(sub));
        }
    }
    for s in all() {
        add(s.label_key());
        add(s.help_key());
    }
    for c in catalogue::commands() {
        add(c.label_key());
        add(c.help_key());
    }
    // Choice labels are shared per family by design; just check the scheme.
    assert_eq!(
        choices::SIZE.label_key("medium"),
        "settings.choice.size.medium"
    );
}

#[test]
fn summaries_are_present() {
    for s in all() {
        assert!(!s.summary.trim().is_empty(), "{}", s.key);
    }
    for c in catalogue::commands() {
        assert!(!c.summary.trim().is_empty(), "{}", c.key);
    }
}

#[test]
fn commands_reset_existing_settings() {
    for c in catalogue::commands() {
        for key in c.resets {
            assert!(
                catalogue::setting(key).is_some(),
                "{} resets unknown {key}",
                c.key
            );
        }
    }
}

/// The palette keys mirror the bundle names in iptv-app's placeholder
/// theme (read at compile time; no dependency on the NSH crate).
#[test]
fn palettes_mirror_the_theme_names() {
    let theme = include_str!("../../iptv-app/src/theme.rs");
    let names = |ctor: &str| -> Vec<String> {
        theme
            .lines()
            .filter_map(|l| l.trim().strip_prefix(ctor))
            .filter_map(|rest| rest.split('"').next())
            .map(|n| n.to_lowercase().replace(' ', "_"))
            .collect()
    };
    assert_eq!(names("a(\""), choices::ACCENT.variants, "accents");
    assert_eq!(names("b(\""), choices::BACKGROUND.variants, "backgrounds");
}

#[test]
fn every_scope_and_effect_is_exercised() {
    let scopes: BTreeSet<Scope> = all().iter().map(|s| s.scope).collect();
    assert_eq!(scopes.len(), 3);
    let effects: Vec<Effect> = all()
        .iter()
        .flat_map(|s| s.depends)
        .map(|d| d.effect)
        .collect();
    assert!(effects.contains(&Effect::Visible) && effects.contains(&Effect::Enabled));
    assert!(all().iter().any(|s| s.flags.requires_restart));
    assert!(all().iter().any(|s| s.flags.advanced));
}

#[test]
fn tree_lists_every_setting_once() {
    let count = |advanced: bool| -> Vec<&str> {
        catalogue::tree(advanced)
            .iter()
            .flat_map(|sec| &sec.groups)
            .flat_map(|g| g.settings.iter().map(|s| s.key))
            .collect()
    };
    let full = count(true);
    let unique: BTreeSet<_> = full.iter().collect();
    assert_eq!(full.len(), all().len());
    assert_eq!(unique.len(), full.len());
    let basic = count(false);
    assert_eq!(
        basic.len(),
        all().iter().filter(|s| !s.flags.advanced).count()
    );
    let commands: usize = catalogue::tree(false)
        .iter()
        .flat_map(|sec| &sec.groups)
        .map(|g| g.commands.len())
        .sum();
    assert_eq!(commands, catalogue::commands().count());
    // Groups follow the declared subsection order, unsectioned first.
    for sec in catalogue::tree(true) {
        let order: Vec<Option<&str>> = sec.groups.iter().map(|g| g.subsection).collect();
        let declared: Vec<Option<&str>> = std::iter::once(None)
            .chain(sec.section.subsections().iter().copied().map(Some))
            .filter(|s| order.contains(s))
            .collect();
        assert_eq!(order, declared, "{}", sec.section.key());
    }
}
