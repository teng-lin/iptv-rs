//! Playlist ↔ EPG channel matching on synthetic channels.

use iptv_epg::*;

fn ch(id: &str, names: &[&str]) -> XmltvChannel {
    XmltvChannel {
        id: id.into(),
        display_names: names.iter().map(|s| s.to_string()).collect(),
        icon: None,
    }
}

struct Pl {
    key: ChannelKey,
    tvg_id: Option<&'static str>,
    name: &'static str,
    tvg_name: Option<&'static str>,
}

fn pl(key: &str, tvg_id: Option<&'static str>, name: &'static str) -> Pl {
    Pl {
        key: key.into(),
        tvg_id,
        name,
        tvg_name: None,
    }
}

fn run(index: &MatchIndex, playlist: &[Pl], overrides: &Overrides) -> MatchResult {
    index.match_channels(
        playlist.iter().map(|p| PlaylistChannel {
            key: &p.key,
            tvg_id: p.tvg_id,
            name: p.name,
            tvg_name: p.tvg_name,
        }),
        overrides,
    )
}

fn first<'a>(r: &'a MatchResult, key: &str) -> Option<&'a EpgRef> {
    r.matches
        .get(&ChannelKey::from(key))
        .and_then(|c| c.first())
}

fn one_source() -> MatchIndex {
    let mut idx = MatchIndex::new();
    let chans = [
        ch("alpha.example", &["Alpha One"]),
        ch("Beta.Example", &["Béta Deux"]),
        ch("gamma.example", &["Gamma"]),
        ch("gamma-plus.example", &["Gamma +1"]),
        ch("delta.example", &["Delta News"]),
        ch("delta2.example", &["Delta News HD"]),
        ch("epsilon.example", &["UK: Epsilon Kids"]),
    ];
    idx.add_source(SourceId(1), 0, chans.iter());
    idx
}

#[test]
fn tvg_id_then_name_with_folding() {
    let idx = one_source();
    let playlist = [
        // tvg-id wins even though the name points elsewhere.
        pl("p1", Some("gamma.example"), "Alpha One"),
        // Case-insensitive id.
        pl("p2", Some("beta.example"), "whatever"),
        // Unknown tvg-id falls back to the name; quality suffix and case fold.
        pl("p3", Some("nope.example"), "ALPHA ONE FHD"),
        // Diacritics and punctuation.
        pl("p4", None, "Beta-Deux"),
        // "+1" stays distinct from the base channel.
        pl("p5", None, "Gamma +1 HD"),
        pl("p6", None, "Gamma"),
        // Playlist tag prefix is removed in the fallback pass.
        pl("p7", None, "US | Alpha One"),
        // EPG-side tag prefix too.
        pl("p8", None, "Epsilon Kids"),
        pl("p9", None, "Unknown Channel"),
    ];
    let r = run(&idx, &playlist, &Overrides::new());
    let id = |k| first(&r, k).map(|c| (c.epg_id.as_str(), c.kind));
    assert_eq!(id("p1"), Some(("gamma.example", MatchKind::TvgId)));
    assert_eq!(id("p2"), Some(("Beta.Example", MatchKind::TvgId)));
    assert_eq!(id("p3"), Some(("alpha.example", MatchKind::Name)));
    assert_eq!(id("p4"), Some(("Beta.Example", MatchKind::Name)));
    assert_eq!(id("p5"), Some(("gamma-plus.example", MatchKind::Name)));
    assert_eq!(id("p6"), Some(("gamma.example", MatchKind::Name)));
    assert_eq!(id("p7"), Some(("alpha.example", MatchKind::Name)));
    assert_eq!(id("p8"), Some(("epsilon.example", MatchKind::Name)));
    assert_eq!(id("p9"), None);

    let rep = &r.report;
    assert_eq!(rep.channels, 9);
    assert_eq!((rep.by_tvg_id, rep.by_name, rep.unmatched), (2, 6, 1));
    assert_eq!(rep.matched(), 8);
    assert_eq!(rep.unmatched_keys, [ChannelKey::from("p9")]);
}

#[test]
fn tvg_name_is_a_second_name() {
    let idx = one_source();
    let mut p = pl("p", None, "Totally Different");
    p.tvg_name = Some("Delta News");
    let r = run(&idx, &[p], &Overrides::new());
    assert_eq!(first(&r, "p").unwrap().epg_id, "delta.example");
}

#[test]
fn ambiguous_names_pick_the_first_and_are_reported() {
    let idx = one_source();
    // "Delta News" and "Delta News HD" fold to the same key.
    let r = run(&idx, &[pl("p", None, "delta news")], &Overrides::new());
    assert_eq!(first(&r, "p").unwrap().epg_id, "delta.example");
    assert_eq!(r.report.ambiguous_name, 1);
}

#[test]
fn overrides_win_over_automatic_matching() {
    let mut idx = one_source();
    idx.add_source(SourceId(2), 5, [ch("alpha.example", &["Alpha One"])].iter());
    let playlist = [
        pl("o1", Some("alpha.example"), "Alpha One"),
        pl("o2", Some("alpha.example"), "Alpha One"),
        pl("o3", Some("alpha.example"), "Alpha One"),
        pl("o4", None, "Alpha One"),
        pl("o5", None, "Alpha One"),
    ];
    let mut ov = Overrides::new();
    ov.insert(
        "o1".into(),
        Override::Epg {
            source: Some(SourceId(1)),
            epg_id: "gamma.example".into(),
        },
    );
    ov.insert(
        "o2".into(),
        Override::Epg {
            source: None,
            epg_id: "alpha.example".into(),
        },
    );
    ov.insert("o3".into(), Override::NoEpg);
    ov.insert(
        "o4".into(),
        Override::Epg {
            source: Some(SourceId(9)),
            epg_id: "later.example".into(),
        },
    );
    ov.insert(
        "o5".into(),
        Override::Epg {
            source: None,
            epg_id: "missing.example".into(),
        },
    );
    let r = run(&idx, &playlist, &ov);
    let o1 = &r.matches[&ChannelKey::from("o1")];
    assert_eq!(o1.len(), 1);
    assert_eq!(
        (o1[0].source, o1[0].epg_id.as_str()),
        (SourceId(1), "gamma.example")
    );
    assert_eq!(o1[0].kind, MatchKind::Override);
    // Any-source override lists every source with that id, by priority.
    let o2: Vec<_> = r.matches[&ChannelKey::from("o2")]
        .iter()
        .map(|c| c.source)
        .collect();
    assert_eq!(o2, [SourceId(2), SourceId(1)]);
    assert!(!r.matches.contains_key(&ChannelKey::from("o3")));
    // A source-specific override is kept even when not resolvable yet.
    assert_eq!(r.matches[&ChannelKey::from("o4")][0].source, SourceId(9));
    assert!(!r.matches.contains_key(&ChannelKey::from("o5")));
    let rep = &r.report;
    assert_eq!(
        (
            rep.by_override,
            rep.disabled,
            rep.override_unresolved,
            rep.unmatched
        ),
        (2, 1, 2, 0)
    );
}

#[test]
fn candidates_follow_source_priority() {
    let mut idx = MatchIndex::new();
    idx.add_source(SourceId(1), 1, [ch("a.example", &["Alpha One"])].iter());
    idx.add_source(SourceId(2), 10, [ch("x.example", &["alpha one hd"])].iter());
    idx.add_source(SourceId(3), 1, [ch("alpha.example", &["Other"])].iter());
    let r = run(
        &idx,
        &[pl("p", Some("alpha.example"), "Alpha One")],
        &Overrides::new(),
    );
    let c: Vec<_> = r.matches[&ChannelKey::from("p")]
        .iter()
        .map(|c| (c.source.0, c.epg_id.as_str(), c.kind))
        .collect();
    assert_eq!(
        c,
        [
            (2, "x.example", MatchKind::Name),
            (1, "a.example", MatchKind::Name),
            (3, "alpha.example", MatchKind::TvgId),
        ]
    );
    // The report counts the kind of the best candidate.
    assert_eq!(r.report.by_name, 1);

    // Replacing a source re-sorts it.
    idx.add_source(SourceId(2), 0, [ch("x.example", &["alpha one hd"])].iter());
    let r = run(&idx, &[pl("p", None, "Alpha One")], &Overrides::new());
    assert_eq!(r.matches[&ChannelKey::from("p")][0].source, SourceId(1));
    idx.remove_source(SourceId(1));
    let r = run(&idx, &[pl("p", None, "Alpha One")], &Overrides::new());
    assert_eq!(r.matches[&ChannelKey::from("p")][0].source, SourceId(2));
}

/// Every candidate `match_channels` returns is a channel the filter wants,
/// and the filter leaves out channels nothing can match.
#[test]
fn match_filter_is_a_superset_of_the_candidates() {
    let feed = [
        ch("alpha.example", &["Alpha One"]),
        ch("Beta.Example", &["Béta Deux"]),
        ch("gamma.example", &["Gamma"]),
        ch("gamma-plus.example", &["Gamma +1"]),
        ch("delta.example", &["Delta News"]),
        ch("delta2.example", &["Delta News HD"]),
        ch("epsilon.example", &["UK: Epsilon Kids"]),
        ch("zeta.example", &["Zeta"]),
        ch("eta.example", &["Eta"]),
        ch("unrelated.example", &["Nothing Here"]),
    ];
    let mut idx = MatchIndex::new();
    idx.add_source(SourceId(1), 0, feed.iter());
    let mut tagged = pl("e", None, "Epsilon Kids");
    tagged.tvg_name = Some("FR: Epsilon Kids");
    let playlist = [
        pl("a", Some(" ALPHA.example "), "Something"),
        pl("b", None, "BETA DEUX"),
        pl("g", Some("missing.example"), "Gamma"),
        pl("d", None, "Delta News HD"),
        tagged,
        pl("z", Some("zeta.example"), "Zeta"),
        pl("h", None, "Eta"),
    ];
    let mut overrides = Overrides::new();
    // An override wins: the playlist channel's own id and name no longer
    // count, the named EPG channel does (declared or not).
    overrides.insert(
        "z".into(),
        Override::Epg {
            source: Some(SourceId(1)),
            epg_id: "undeclared.example".into(),
        },
    );
    overrides.insert("h".into(), Override::NoEpg);
    let result = run(&idx, &playlist, &overrides);
    let filter = MatchFilter::new(
        playlist.iter().map(|p| PlaylistChannel {
            key: &p.key,
            tvg_id: p.tvg_id,
            name: p.name,
            tvg_name: p.tvg_name,
        }),
        &overrides,
    );
    for cand in result.matches.values().flatten() {
        let declared = feed.iter().find(|c| c.id == cand.epg_id);
        let wanted = match declared {
            Some(c) => filter.wants_channel(cand.source, c),
            None => filter.wants_id(cand.source, &cand.epg_id),
        };
        assert!(wanted, "{cand:?}");
    }
    let wanted: Vec<&str> = feed
        .iter()
        .filter(|c| filter.wants_channel(SourceId(1), c))
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(
        wanted,
        [
            "alpha.example",
            "Beta.Example",
            "gamma.example",
            // Quality tags fold away: both Delta channels share the key.
            "delta.example",
            "delta2.example",
            "epsilon.example"
        ]
    );
    assert!(filter.wants_id(SourceId(1), "undeclared.example"));
    assert!(!filter.wants_id(SourceId(2), "undeclared.example"));
}
