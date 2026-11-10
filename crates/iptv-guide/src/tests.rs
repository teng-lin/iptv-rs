use iptv_core::guide::{Focus, Move, Program, Schedule};
use iptv_epg::{
    ChannelKey, GuideStore, MatchIndex, Overrides, ParseOptions, PlaylistChannel, SourceBuilder,
    SourceId, StoreOptions, format_xmltv_time, parse_xmltv,
};
use iptv_providers::m3u::parse_m3u;

use super::{EpgSchedule, GuideChannel, NO_INFORMATION};

/// Hour-aligned "now" for readable minutes.
const NOW: i64 = 1_800_000_000;

/// Channel `a.test` has programmes around now with a gap; `b.test` has none.
fn xmltv() -> String {
    let p = |from: i64, to: i64, title: &str| {
        format!(
            "<programme channel=\"a.test\" start=\"{}\" stop=\"{}\"><title>{title}</title><desc>About {title}.</desc></programme>",
            format_xmltv_time(NOW + from * 60),
            format_xmltv_time(NOW + to * 60),
        )
    };
    format!(
        "<?xml version=\"1.0\"?><tv><channel id=\"a.test\"><display-name>Alpha</display-name></channel>{}{}{}</tv>",
        p(-30, 30, "Alpha News"),
        p(30, 60, "Alpha Talk"),
        p(90, 120, "Alpha Late"),
    )
}

fn store_for(keys: &[(&ChannelKey, Option<&str>, &str)]) -> GuideStore {
    store_with(keys, true)
}

fn store_with(keys: &[(&ChannelKey, Option<&str>, &str)], descriptions: bool) -> GuideStore {
    let parsed =
        parse_xmltv(xmltv().as_bytes(), ParseOptions::default()).expect("synthetic xmltv parses");
    let mut builder = SourceBuilder::new(
        SourceId(1),
        0,
        StoreOptions {
            store_descriptions: descriptions,
            ..Default::default()
        },
        NOW,
    );
    parsed
        .channels
        .into_iter()
        .for_each(|c| builder.push_channel(c));
    parsed
        .programmes
        .into_iter()
        .for_each(|p| builder.push_programme(p));
    let source = builder.finish();
    let mut index = MatchIndex::new();
    index.add_source(source.id(), source.priority(), source.channels());
    let matched = index.match_channels(
        keys.iter().map(|(key, tvg_id, name)| PlaylistChannel {
            key,
            tvg_id: *tvg_id,
            name,
            tvg_name: None,
        }),
        &Overrides::default(),
    );
    let mut store = GuideStore::new();
    store.replace_source(source);
    store.set_matches(matched.matches);
    store
}

fn schedule() -> EpgSchedule {
    let (a, b) = (ChannelKey::new("a"), ChannelKey::new("b"));
    let store = store_for(&[(&a, Some("a.test"), "Alpha"), (&b, None, "Beta")]);
    let ch = |key: ChannelKey, name: &str, days| GuideChannel {
        key,
        name: name.to_owned(),
        catchup_days: days,
    };
    EpgSchedule::new(
        vec![("News".to_owned(), vec![ch(a, "Alpha", 3), ch(b, "Beta", 0)])],
        store,
        NOW,
    )
}

#[test]
fn real_programmes_map_to_guide_minutes_with_titles_and_descriptions() {
    let s = schedule();
    let p = s.program_at(0, 0);
    assert_eq!(
        p,
        Program {
            start: -30,
            end: 30
        }
    );
    assert_eq!(s.title(0, p), "Alpha News");
    assert_eq!(s.description(0, p).as_deref(), Some("About Alpha News."));
    assert_eq!(s.catchup_days(0), 3);
}

#[test]
fn descriptions_left_out_of_memory_come_from_the_lookup() {
    let a = ChannelKey::new("a");
    let store = store_with(&[(&a, Some("a.test"), "Alpha")], false);
    let ch = GuideChannel {
        key: a,
        name: "Alpha".to_owned(),
        catchup_days: 0,
    };
    let plain = EpgSchedule::new(vec![("News".to_owned(), vec![ch.clone()])], store, NOW);
    let p = plain.program_at(0, 0);
    assert_eq!(plain.description(0, p), None);
    let store = store_with(&[(&ch.key, Some("a.test"), "Alpha")], false);
    let lazy = EpgSchedule::new(vec![("News".to_owned(), vec![ch])], store, NOW)
        .with_description_lookup(|key, start| Some(format!("{key} at {}", start - NOW)));
    assert_eq!(lazy.description(0, p).as_deref(), Some("a at -1800"));
    // No programme there: nothing to look up.
    assert_eq!(lazy.description(0, lazy.program_at(0, 70)), None);
}

#[test]
fn gaps_are_filled_with_slots_clipped_to_neighbours() {
    let s = schedule();
    let gap = s.program_at(0, 70);
    assert_eq!(
        gap,
        Program { start: 60, end: 90 },
        "between Alpha Talk and Alpha Late"
    );
    assert_eq!(s.title(0, gap), NO_INFORMATION);
    assert_eq!(
        s.program_at(1, 10),
        Program { start: 0, end: 60 },
        "no EPG: hour slots"
    );
}

#[test]
fn programmes_in_a_window_are_contiguous() {
    let s = schedule();
    for ch in 0..2 {
        let ps = s.programs_between(ch, -30, 150);
        assert!(
            ps.first().unwrap().start <= -30 && ps.last().unwrap().end >= 120,
            "{ps:?}"
        );
        for w in ps.windows(2) {
            assert_eq!(w[0].end, w[1].start, "channel {ch}: {ps:?}");
        }
    }
    let alpha = s.programs_between(0, -30, 150);
    assert_eq!(
        alpha,
        [
            Program {
                start: -30,
                end: 30
            },
            Program { start: 30, end: 60 },
            Program { start: 60, end: 90 },
            Program {
                start: 90,
                end: 120
            },
        ]
    );
}

#[test]
fn the_focus_model_navigates_real_data_and_gaps() {
    let s = schedule();
    let mut f = Focus::default();
    f.apply(Move::Right, &s);
    assert_eq!(f.program(&s), Program { start: 30, end: 60 });
    f.apply(Move::Right, &s);
    assert_eq!(
        f.program(&s),
        Program { start: 60, end: 90 },
        "into the gap"
    );
    f.apply(Move::Down, &s);
    assert_eq!(f.channel, 1);
    assert_eq!(
        f.program(&s),
        Program {
            start: 60,
            end: 120
        },
        "same time column on the EPG-less channel"
    );
    f.apply(Move::Down, &s);
    assert_eq!(f.channel, 1, "clamped to the last channel");
}

#[test]
fn a_catalog_becomes_grouped_channels() {
    let m3u = parse_m3u(
        "#EXTM3U\n\
         #EXTINF:-1 tvg-id=\"a.test\" group-title=\"News\" catchup=\"default\" catchup-days=\"2\" catchup-source=\"http://archive.example.com/a?utc={utc}\",Alpha\n\
         http://live.example.com/a.m3u8\n\
         #EXTINF:-1 group-title=\"Kids\",Kappa\n\
         http://live.example.com/k.m3u8\n\
         #EXTINF:-1 group-title=\"News\",Beta\n\
         http://live.example.com/b.m3u8\n\
         #EXTINF:-1,Loose\n\
         http://live.example.com/l.m3u8\n",
    );
    let s = EpgSchedule::from_catalog(&m3u.groups, &m3u.channels.items, GuideStore::new(), NOW);
    let names: Vec<String> = (0..s.group_count()).map(|g| s.group_name(g)).collect();
    assert_eq!(names, ["News", "Kids", "Other"]);
    assert_eq!(s.group_start(1), 2);
    let channels: Vec<String> = (0..s.channel_count()).map(|c| s.channel_name(c)).collect();
    assert_eq!(channels, ["Alpha", "Beta", "Kappa", "Loose"]);
    assert_eq!(s.catchup_days(0), 2);
    assert_eq!(s.catchup_days(1), 0);
}
