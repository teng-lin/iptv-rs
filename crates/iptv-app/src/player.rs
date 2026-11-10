//! The playback driver: resolves the playing channel to a stream (off the
//! UI thread when the provider needs a round trip), feeds it to the one
//! persistent player (`video.rs`), and runs the core's health machine:
//! retry with backoff for live streams, stall detection, the error and
//! waiting overlays (`player_panels::status`). Also previous channel,
//! back to live, and the last channel on startup.
//!
//! The last channel starts after the startup refresh of a stored source
//! (never under the first-run wizard). It is a one-line file, `last_channel` in
//! [`crate::data::data_dir`], holding the channel's content key (never a
//! list index). It moves to the store's history once the store keeps one
//! for playback.

use std::rc::Rc;
use std::time::Duration;

use futures_channel::oneshot;
use iptv_core::playback::{ChannelHistory, ErrorKind, Health, PlaySource, PlayerState, Retry};
use iptv_guide::EpgSchedule;
use iptv_guide::playback::{Clock, PlaybackContext, Resolution, StalkerLink, Unavailable};
use iptv_import::{CancelToken, SourceMeta};
use iptv_net::{NetClient, NetConfig};
use iptv_providers::model::SourceConfig;
use iptv_providers::xtream::StreamFormat;
use nsh_core::prelude::*;

use crate::guide::Schedule;
use crate::{Playback, Ui, ViewMode};

const LAST_CHANNEL_FILE: &str = "last_channel";

/// Playback state beside [`Ui`]'s `playing`/`playback` (signals are `Copy`).
#[derive(Clone, Copy)]
pub struct Player {
    pub health: Signal<Health>,
    /// What the player has open (`None` until resolved).
    pub source: Signal<Option<PlaySource>>,
    /// Bumped on every open, so a retry reopens even the same URL.
    pub token: Signal<u64>,
    pub history: Signal<ChannelHistory>,
    /// The playing channel's key (live or catch-up): finds it again in a
    /// new schedule.
    key: Signal<Option<String>>,
    /// The last channel still starts on its own until something plays.
    resume: Signal<bool>,
    /// The app's owner: retry and stall timers outlive the effect or
    /// handler that armed them.
    owner: Owner,
}

impl Player {
    pub fn new() -> Self {
        Self {
            health: signal(Health::default()),
            source: signal(None),
            token: signal(0),
            history: signal(ChannelHistory::default()),
            key: signal(None),
            resume: signal(true),
            owner: Owner::current(),
        }
    }
}

/// The playback context of an imported source: what its streams need from
/// it (headers, account, session). Attach it with
/// `EpgSchedule::with_playback`.
pub fn context_for(config: &SourceConfig, meta: &SourceMeta) -> Option<PlaybackContext> {
    match (config, meta) {
        (SourceConfig::M3u(c), _) => Some(PlaybackContext::m3u(c)),
        (SourceConfig::Xtream(c), SourceMeta::Xtream { account, .. }) => {
            PlaybackContext::xtream(c, account.as_ref(), StreamFormat::Auto).ok()
        }
        (SourceConfig::Stalker(_), SourceMeta::Stalker { session }) => {
            Some(PlaybackContext::stalker((**session).clone()))
        }
        _ => None,
    }
}

/// What a stored source's streams need before its refresh ran: no account
/// details yet (HLS for Xtream) and no portal session, so a Stalker
/// source plays only once it imported.
pub fn stored_context(config: &SourceConfig) -> Option<PlaybackContext> {
    match config {
        SourceConfig::M3u(c) => Some(PlaybackContext::m3u(c)),
        SourceConfig::Xtream(c) => PlaybackContext::xtream(c, None, StreamFormat::Auto).ok(),
        _ => None,
    }
}

/// `f` after `delay`, under the app's owner.
fn later(ui: Ui, delay: Duration, f: impl FnOnce() + 'static) {
    ui.player.owner.run(|| {
        let _ = set_timeout(delay, f);
    });
}

/// The viewer's clock, with the local UTC offset at `at` (the programme's
/// start for catch-up: date placeholders describe that instant).
fn clock(at: Option<i64>) -> Clock {
    let now = crate::time::wall_unix();
    Clock {
        now,
        local_offset_secs: crate::time::offset_minutes(at.unwrap_or(now)) * 60,
    }
}

fn real_schedule(s: &Rc<dyn Schedule>) -> Option<&EpgSchedule> {
    s.as_any()?.downcast_ref::<EpgSchedule>()
}

/// Called by [`Ui::play`] after it set the channel and playback: opens the
/// stream with a fresh health session.
pub fn start(ui: &Ui) {
    let ui = *ui;
    let Some(channel) = ui.playing.get_untracked() else {
        stop(ui);
        return;
    };
    let live = ui.playback.get_untracked() == Playback::Live;
    let now = ui.now_ms();
    let mut session = 0;
    ui.player.health.update(|h| session = h.start(live, now));
    ui.player.resume.set(false);
    let key = crate::schedule().channel_key(channel);
    ui.player.key.set(key.clone());
    if live && let Some(key) = key {
        let changed = ui
            .player
            .history
            .with_untracked(|h| h.current() != Some(&key));
        ui.player.history.update(|h| h.played(&key));
        if changed {
            save_last_channel(&key);
        }
    }
    open(ui, session);
}

/// Nothing plays: the player closes.
pub fn stop(ui: Ui) {
    ui.player.health.update(Health::stop);
    ui.player.source.set(None);
    crate::media::reset(ui);
}

/// Resolves the playing channel and hands the stream to the player.
fn open(ui: Ui, session: u64) {
    let Some(channel) = ui.playing.get_untracked() else {
        stop(ui);
        return;
    };
    let schedule = crate::schedule();
    let (archive, offset) = match ui.playback.get_untracked() {
        Playback::Live => (None, 0),
        Playback::CatchUp { start, offset } => (Some(schedule.program_at(channel, start)), offset),
    };
    let Some(epg) = real_schedule(&schedule) else {
        // The synthetic guide plays the local test streams.
        let url = match ui.playback.get_untracked() {
            Playback::Live => crate::video::stream_url(channel),
            Playback::CatchUp { start, offset } => {
                crate::video::catchup_url(channel, start, offset)
            }
        };
        load(ui, session, PlaySource::new(url));
        return;
    };
    let at = archive.map(|p| crate::guide::unix_of(&*schedule, p.start));
    match epg.resolve_from(channel, archive, i64::from(offset), clock(at)) {
        Resolution::Play(source) => load(ui, session, source),
        Resolution::StalkerLink(link) => resolve_link(ui, session, link, schedule.clone()),
        Resolution::Unavailable(why) => {
            let detail = match why {
                Unavailable::NoChannel => "no channel".to_owned(),
                Unavailable::NeedsAccount => "no account".to_owned(),
                Unavailable::NoArchive(e) => format!("{e:?}"),
            };
            fail(ui, session, ErrorKind::Unavailable, detail);
        }
    }
}

fn load(ui: Ui, session: u64, source: PlaySource) {
    if ui.player.health.with_untracked(|h| h.session()) != session {
        return; // the viewer moved on
    }
    log::info!(
        "iptv player open: session {session}, {} header(s)",
        source.headers.len()
    );
    ui.player.token.update(|t| *t += 1);
    ui.position_ms.set(0);
    // A new stream: its own tracks, from the default choices.
    crate::media::reset(ui);
    ui.player.source.set(Some(source));
    let now = ui.now_ms();
    ui.player.health.update(|h| h.resolved(now));
    watch_stall(ui);
}

/// A Stalker `create_link` on its own thread; the result comes back to the
/// UI thread and is dropped if the viewer has moved on.
fn resolve_link(ui: Ui, session: u64, link: Box<StalkerLink>, schedule: Rc<dyn Schedule>) {
    let StalkerLink {
        session: mut portal,
        cmd,
        channel_headers,
    } = *link;
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "runtime".to_owned())?;
            runtime.block_on(async {
                let net = NetClient::new(NetConfig::default()).map_err(|_| "network".to_owned())?;
                iptv_import::stalker_link(&net, &mut portal, &cmd, false, &CancelToken::new())
                    .await
                    .map_err(|e| format!("{:?}", e.kind()))
            })
        })();
        let _ = tx.send((result, portal));
    });
    let _ = ui.player.owner.run(|| {
        spawn(async move {
            let Ok((result, portal)) = rx.await else {
                return;
            };
            // Keep a refreshed token for the next link, if the data is the same.
            if Rc::ptr_eq(&schedule, &crate::schedule())
                && let Some(ctx) = real_schedule(&schedule).and_then(EpgSchedule::playback)
            {
                ctx.update_stalker_session(portal.clone());
            }
            match result {
                Ok(url) => load(
                    ui,
                    session,
                    StalkerLink::source(&portal, url, &channel_headers),
                ),
                Err(kind) => fail(ui, session, ErrorKind::Resolve, kind),
            }
        })
    });
}

fn fail(ui: Ui, session: u64, kind: ErrorKind, detail: String) {
    if ui.player.health.with_untracked(|h| h.session()) != session {
        return;
    }
    log::warn!("iptv player: {kind:?} ({detail})");
    let now = ui.now_ms();
    let mut retry = None;
    ui.player
        .health
        .update(|h| retry = h.on_error(kind, detail, now));
    if let Some(r) = retry {
        schedule_retry(ui, r);
    }
}

fn schedule_retry(ui: Ui, retry: Retry) {
    log::info!(
        "iptv player retry {} in {} ms",
        retry.attempt,
        retry.at_ms.saturating_sub(ui.now_ms())
    );
    let wait = retry.at_ms.saturating_sub(ui.now_ms());
    later(ui, Duration::from_millis(wait), move || {
        let now = ui.now_ms();
        let mut due = false;
        ui.player.health.update(|h| due = h.due(retry, now));
        if due {
            open(ui, retry.session);
        }
    });
}

/// Arms one stall check at the current deadline (each wait gets its own; a
/// check whose deadline moved does nothing).
fn watch_stall(ui: Ui) {
    let Some(at) = ui.player.health.with_untracked(|h| h.stall_deadline()) else {
        return;
    };
    let wait = at.saturating_sub(ui.now_ms());
    later(ui, Duration::from_millis(wait), move || {
        let now = ui.now_ms();
        let mut retry = None;
        ui.player.health.update(|h| retry = h.tick(now));
        if let Some(r) = retry {
            log::warn!("iptv player: stalled");
            schedule_retry(ui, r);
        }
    });
}

/// The host's `state` event.
pub fn on_state(ui: Ui, name: &str) {
    let Some(state) = PlayerState::parse(name) else {
        return;
    };
    let now = ui.now_ms();
    let before = ui.player.health.with_untracked(|h| h.stall_deadline());
    let mut retry = None;
    ui.player.health.update(|h| retry = h.on_state(state, now));
    if let Some(r) = retry {
        schedule_retry(ui, r);
    } else if ui.player.health.with_untracked(|h| h.stall_deadline()) != before {
        watch_stall(ui);
    }
}

/// The host's `error` event: Media3's code name and the HTTP status.
pub fn on_error(ui: Ui, code: &str, http: Option<u16>) {
    let kind = ErrorKind::from_player_code(code, http);
    let session = ui.player.health.with_untracked(|h| h.session());
    let detail = match http {
        Some(status) => format!("{code} {status}"),
        None => code.to_owned(),
    };
    fail(ui, session, kind, detail);
}

pub fn failed(ui: &Ui) -> bool {
    ui.playing.get_untracked().is_some()
        && ui.player.health.with_untracked(|h| h.failure().is_some())
}

/// The viewer's Retry: starts over with a fresh attempt count.
pub fn retry(ui: Ui) {
    let now = ui.now_ms();
    let mut session = 0;
    ui.player.health.update(|h| session = h.retry_now(now));
    open(ui, session);
}

/// Plays the channel before the current one (again: back).
pub fn previous_channel(ui: Ui) {
    let Some(key) = ui
        .player
        .history
        .with_untracked(|h| h.previous().map(str::to_owned))
    else {
        return;
    };
    let Some(channel) = crate::schedule().find_channel(&key) else {
        return;
    };
    show_channel(ui, channel);
    ui.play(channel, Playback::Live);
    if ui.mode.get_untracked() == ViewMode::Fullscreen {
        ui.update_panels(|p, now| p.show_info(now));
    }
}

/// From catch-up back to the live stream of the playing channel.
pub fn go_live(ui: Ui) {
    if let (Some(channel), Playback::CatchUp { .. }) =
        (ui.playing.get_untracked(), ui.playback.get_untracked())
    {
        ui.play(channel, Playback::Live);
        if ui.mode.get_untracked() == ViewMode::Fullscreen {
            ui.update_panels(|p, now| p.show_info(now));
        }
    }
}

fn show_channel(ui: Ui, channel: usize) {
    ui.focus
        .update(|f| f.show_channel(channel, &*crate::schedule()));
}

fn last_channel_path() -> std::path::PathBuf {
    crate::data::data_dir().join(LAST_CHANNEL_FILE)
}

fn save_last_channel(key: &str) {
    let path = last_channel_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, key) {
        log::warn!(
            "iptv player: could not save the last channel ({})",
            e.kind()
        );
    }
}

fn load_last_channel() -> Option<String> {
    let key = std::fs::read_to_string(last_channel_path()).ok()?;
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_owned())
}

/// Data changes keep the playing channel's row (rows are rebuilt by
/// identity); stall checks pause while paused.
pub fn bind(ui: Ui) {
    effect(move || {
        ui.data_version.get();
        // Out of the effect: following the row writes signals.
        later(ui, Duration::ZERO, move || follow_playing(ui));
    });
    effect(move || {
        let paused = ui.paused.get();
        let now = ui.now_ms();
        untracked(|| {
            ui.player.health.update(|h| h.set_paused(paused, now));
            if !paused {
                watch_stall(ui);
            }
        });
    });
}

fn follow_playing(ui: Ui) {
    let Some(playing) = ui.playing.get_untracked() else {
        return;
    };
    let key = ui.player.key.get_untracked();
    match key.and_then(|k| crate::schedule().find_channel(&k)) {
        Some(row) => {
            if row != playing {
                ui.playing.set(Some(row));
            }
            // The new schedule reset the focus: put it back on what plays.
            show_channel(ui, row);
        }
        // Another source (or the channel left it): its row now means
        // another channel, so nothing plays.
        None => {
            log::info!("iptv player: the playing channel is not in the new guide; stopping");
            ui.playing.set(None);
            ui.player.key.set(None);
            stop(ui);
            if ui.mode.get_untracked() != ViewMode::Guide {
                ui.mode.set(ViewMode::Guide);
            }
        }
    }
}

/// `data.rs` put an imported source's schedule on screen. After the
/// startup refresh of a stored source (`refresh`), and with no wizard open,
/// the last channel starts once, unless something already plays.
pub fn source_ready(ui: Ui, refresh: bool) {
    if !refresh
        || !ui.player.resume.get_untracked()
        || ui.playing.get_untracked().is_some()
        || ui.wizard.with_untracked(Option::is_some)
    {
        return;
    }
    let Some(channel) = load_last_channel().and_then(|k| crate::schedule().find_channel(&k)) else {
        return;
    };
    log::info!("iptv player: resuming the last channel");
    show_channel(ui, channel);
    ui.play(channel, Playback::Live);
    ui.mode.set(ViewMode::Fullscreen);
    ui.update_panels(|p, now| p.show_info(now));
}
