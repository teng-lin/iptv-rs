//! The playback driver: resolves the playing channel to a stream (off the
//! UI thread when the provider needs a round trip), feeds it to its player
//! surface (`video.rs`), and runs the core's health machine per surface:
//! retry with backoff for live streams, stall detection, the error and
//! waiting overlays (`player_panels::status`, `multiview_view`). Also
//! previous channel, back to live, and the last channel on startup.
//!
//! Surface 0 is the persistent player of the guide's preview and full
//! screen; it plays [`Ui::playing`] as [`Ui::playback`] says. Multiview
//! (`multiview.rs`) adds surfaces 1 to [`SURFACES`] − 1, each playing its
//! [`Stream::channel`] live; every surface has its own [`Stream`]: stream,
//! retries, position, size and tracks.
//!
//! The last channel starts after the startup refresh of a stored source
//! (never under the first-run wizard). Each playlist has its own: a
//! one-line file, `last_channel-<source id>` in [`crate::data::data_dir`],
//! holding the channel's content key (never a list index); builds before
//! that kept one `last_channel` file, still read when a playlist has none.
//! It moves to the store's history once the store keeps one for playback.

use std::rc::Rc;
use std::time::Duration;

use futures_channel::oneshot;
use iptv_core::playback::{ChannelHistory, ErrorKind, Health, PlaySource, PlayerState, Retry};
use iptv_core::player_info::StreamFormat as Format;
use iptv_core::tracks::{Choice, Tracks};
use iptv_guide::EpgSchedule;
use iptv_guide::playback::{Clock, PlaybackContext, Resolution, StalkerLink, Unavailable};
use iptv_import::{CancelToken, SourceMeta};
use iptv_net::NetClient;
use iptv_providers::model::SourceConfig;
use iptv_providers::xtream::StreamFormat;
use iptv_store_sqlite::SourceId;
use nsh_core::prelude::*;

use crate::guide::Schedule;
use crate::{Playback, Ui, ViewMode};

const LAST_CHANNEL_FILE: &str = "last_channel";

/// Player surfaces: the full-screen one and multiview's others.
pub const SURFACES: usize = iptv_core::multiview::MAX_SURFACES;

/// One player surface's playback (signals are `Copy`).
#[derive(Clone, Copy)]
pub struct Stream {
    pub health: Signal<Health>,
    /// What the player has open (`None` until resolved).
    pub source: Signal<Option<PlaySource>>,
    /// Bumped on every open, so a retry reopens even the same URL.
    pub token: Signal<u64>,
    /// The open stream is live (not catch-up): the player bridges
    /// connections the server closes.
    pub live: Signal<bool>,
    /// The open is a retry of a stream that played: the player keeps its
    /// last frame instead of going black.
    pub reconnect: Signal<bool>,
    /// The playing channel's key (live or catch-up): finds it again in a
    /// new schedule.
    pub key: Signal<Option<String>>,
    /// What surfaces other than 0 play (catalog), always live; surface 0
    /// plays [`Ui::playing`].
    pub channel: Signal<Option<usize>>,
    /// The video's size, as the player reported it.
    pub size: Signal<Option<(u32, u32)>>,
    /// Where the open stream is (ms from where it was opened), as the
    /// player last reported.
    pub position_ms: Signal<u64>,
    /// What the open stream offers and what the viewer chose (`media.rs`).
    pub tracks: Signal<Tracks>,
    pub choice: Signal<Choice>,
    /// Frame rate and audio channels; the size is [`Stream::size`].
    pub format: Signal<Format>,
}

impl Stream {
    fn new() -> Self {
        Stream {
            health: signal(Health::default()),
            source: signal(None),
            token: signal(0),
            live: signal(true),
            reconnect: signal(false),
            key: signal(None),
            channel: signal(None),
            size: signal(None),
            position_ms: signal(0),
            tracks: signal(Tracks::default()),
            choice: signal(Choice::default()),
            format: signal(Format::default()),
        }
    }
}

/// Playback state beside [`Ui`]'s `playing`/`playback` (signals are `Copy`).
#[derive(Clone, Copy)]
pub struct Player {
    /// By surface; 0 is the full-screen player.
    pub streams: [Stream; SURFACES],
    pub history: Signal<ChannelHistory>,
    /// The last channel still starts on its own until something plays.
    resume: Signal<bool>,
    /// The app's owner: retry and stall timers outlive the effect or
    /// handler that armed them.
    owner: Owner,
}

impl Player {
    pub fn new() -> Self {
        Self {
            streams: std::array::from_fn(|_| Stream::new()),
            history: signal(ChannelHistory::default()),
            resume: signal(true),
            owner: Owner::current(),
        }
    }

    /// The full-screen player's (surface 0).
    pub fn main(&self) -> Stream {
        self.streams[0]
    }

    pub fn stream(&self, surface: usize) -> Stream {
        self.streams[surface.min(SURFACES - 1)]
    }
}

/// What `surface` plays: surface 0 the playing channel as `playback`
/// says, the others their channel live.
pub fn target(ui: Ui, surface: usize) -> Option<(usize, Playback)> {
    if surface == 0 {
        Some((ui.playing.get_untracked()?, ui.playback.get_untracked()))
    } else {
        let channel = ui.player.stream(surface).channel.get_untracked()?;
        Some((channel, Playback::Live))
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
    let main = ui.player.main();
    main.health.update(|h| session = h.start(live, now));
    ui.player.resume.set(false);
    let key = crate::schedule().channel_key(channel);
    main.key.set(key.clone());
    if live && let Some(key) = key {
        let changed = ui
            .player
            .history
            .with_untracked(|h| h.current() != Some(&key));
        ui.player.history.update(|h| h.played(&key));
        if changed {
            save_last_channel(ui.library.shown.get_untracked(), &key);
            crate::favorites::played(&key);
        }
    }
    open(ui, 0, session);
}

/// Surface `surface` (not 0) plays `channel` live, with a fresh health
/// session; the channel history is the full-screen player's only.
pub fn start_surface(ui: Ui, surface: usize, channel: usize) {
    if surface == 0 {
        ui.play(channel, Playback::Live);
        return;
    }
    let stream = ui.player.stream(surface);
    let now = ui.now_ms();
    let mut session = 0;
    stream.health.update(|h| session = h.start(true, now));
    stream.channel.set(Some(channel));
    stream.key.set(crate::schedule().channel_key(channel));
    log::info!("iptv player {surface}: starting");
    open(ui, surface, session);
}

/// Nothing plays: the player closes.
pub fn stop(ui: Ui) {
    close(ui, 0);
}

/// Surface `surface` plays nothing (surface 0 keeps its channel's key;
/// [`stop`] is its own).
pub fn stop_surface(ui: Ui, surface: usize) {
    if surface == 0 {
        ui.playing.set(None);
        stop(ui);
        return;
    }
    let stream = ui.player.stream(surface);
    stream.channel.set(None);
    stream.key.set(None);
    close(ui, surface);
}

fn close(ui: Ui, surface: usize) {
    let stream = ui.player.stream(surface);
    stream.health.update(Health::stop);
    stream.source.set(None);
    if stream.size.with_untracked(Option::is_some) {
        stream.size.set(None);
    }
    crate::media::reset(ui, surface);
}

/// Resolves what `surface` plays and hands the stream to its player.
fn open(ui: Ui, surface: usize, session: u64) {
    let Some((channel, playback)) = target(ui, surface) else {
        close(ui, surface);
        return;
    };
    let schedule = crate::schedule();
    let (archive, offset) = match playback {
        Playback::Live => (None, 0),
        Playback::CatchUp { start, offset } => (Some(schedule.program_at(channel, start)), offset),
    };
    let Some(epg) = real_schedule(&schedule) else {
        // The synthetic guide plays the local test streams.
        let url = match playback {
            Playback::Live => crate::video::stream_url(channel),
            Playback::CatchUp { start, offset } => {
                crate::video::catchup_url(channel, start, offset)
            }
        };
        load(ui, surface, session, PlaySource::new(url));
        return;
    };
    let at = archive.map(|p| crate::guide::unix_of(&*schedule, p.start));
    match epg.resolve_from(channel, archive, i64::from(offset), clock(at)) {
        Resolution::Play(source) => load(ui, surface, session, source),
        Resolution::StalkerLink(link) => resolve_link(ui, surface, session, link, schedule.clone()),
        Resolution::Unavailable(why) => {
            let detail = match why {
                Unavailable::NoChannel => "no channel".to_owned(),
                Unavailable::NeedsAccount => "no account".to_owned(),
                Unavailable::NoArchive(e) => format!("{e:?}"),
            };
            fail(ui, surface, session, ErrorKind::Unavailable, detail);
        }
    }
}

fn load(ui: Ui, surface: usize, session: u64, mut source: PlaySource) {
    let stream = ui.player.stream(surface);
    if stream.health.with_untracked(|h| h.session()) != session {
        return; // the viewer moved on
    }
    crate::settings::add_user_agent(&mut source.headers, crate::settings::user_agent());
    let (live, reconnect) = stream
        .health
        .with_untracked(|h| (h.live(), h.reconnecting()));
    log::info!(
        "iptv player {surface} open: session {session}, {} header(s){}",
        source.headers.len(),
        if reconnect { ", reconnecting" } else { "" }
    );
    stream.live.set(live);
    stream.reconnect.set(reconnect);
    stream.token.update(|t| *t += 1);
    stream.position_ms.set(0);
    // A new stream: its own tracks, from the default choices.
    crate::media::reset(ui, surface);
    stream.source.set(Some(source));
    let now = ui.now_ms();
    stream.health.update(|h| h.resolved(now));
    watch_stall(ui, surface);
}

/// A Stalker `create_link` on its own thread; the result comes back to the
/// UI thread and is dropped if the viewer has moved on.
fn resolve_link(
    ui: Ui,
    surface: usize,
    session: u64,
    link: Box<StalkerLink>,
    schedule: Rc<dyn Schedule>,
) {
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
                let net = NetClient::new(crate::settings::net_config())
                    .map_err(|_| "network".to_owned())?;
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
                    surface,
                    session,
                    StalkerLink::source(&portal, url, &channel_headers),
                ),
                Err(kind) => fail(ui, surface, session, ErrorKind::Resolve, kind),
            }
        })
    });
}

fn fail(ui: Ui, surface: usize, session: u64, kind: ErrorKind, detail: String) {
    let health = ui.player.stream(surface).health;
    if health.with_untracked(|h| h.session()) != session {
        return;
    }
    log::warn!("iptv player {surface}: {kind:?} ({detail})");
    let now = ui.now_ms();
    let mut retry = None;
    health.update(|h| retry = h.on_error(kind, detail, now));
    if let Some(r) = retry {
        schedule_retry(ui, surface, r);
    }
}

fn schedule_retry(ui: Ui, surface: usize, retry: Retry) {
    log::info!(
        "iptv player {surface} retry {} in {} ms",
        retry.attempt,
        retry.at_ms.saturating_sub(ui.now_ms())
    );
    let wait = retry.at_ms.saturating_sub(ui.now_ms());
    watch_indicator(ui, surface);
    later(ui, Duration::from_millis(wait), move || {
        let now = ui.now_ms();
        let mut due = false;
        ui.player
            .stream(surface)
            .health
            .update(|h| due = h.due(retry, now));
        if due {
            open(ui, surface, retry.session);
        }
    });
}

/// Shows the waiting indicator once a quiet reconnect has taken long
/// enough (`Health::indicator`).
fn watch_indicator(ui: Ui, surface: usize) {
    let health = ui.player.stream(surface).health;
    let Some(at) = health.with_untracked(|h| h.indicator_due()) else {
        return;
    };
    let wait = at.saturating_sub(ui.now_ms());
    later(ui, Duration::from_millis(wait), move || {
        if health.with_untracked(|h| h.indicator_due()) == Some(at) {
            // Nothing changed but the time: the overlays read it again.
            health.update(|_| {});
        }
    });
}

/// Arms one stall check at the current deadline (each wait gets its own; a
/// check whose deadline moved does nothing).
fn watch_stall(ui: Ui, surface: usize) {
    let health = ui.player.stream(surface).health;
    let Some(at) = health.with_untracked(|h| h.stall_deadline()) else {
        return;
    };
    let wait = at.saturating_sub(ui.now_ms());
    later(ui, Duration::from_millis(wait), move || {
        let now = ui.now_ms();
        let mut retry = None;
        health.update(|h| retry = h.tick(now));
        if let Some(r) = retry {
            log::warn!("iptv player {surface}: stalled");
            schedule_retry(ui, surface, r);
        }
    });
}

/// The host's `state` event from `surface`.
pub fn on_state(ui: Ui, surface: usize, name: &str) {
    let Some(state) = PlayerState::parse(name) else {
        return;
    };
    let health = ui.player.stream(surface).health;
    let now = ui.now_ms();
    let before = health.with_untracked(|h| h.stall_deadline());
    let mut retry = None;
    health.update(|h| retry = h.on_state(state, now));
    if let Some(r) = retry {
        schedule_retry(ui, surface, r);
    } else if health.with_untracked(|h| h.stall_deadline()) != before {
        watch_stall(ui, surface);
    }
}

/// The host's `error` event from `surface`: Media3's code name and the
/// HTTP status.
pub fn on_error(ui: Ui, surface: usize, code: &str, http: Option<u16>) {
    let kind = ErrorKind::from_player_code(code, http);
    let session = ui
        .player
        .stream(surface)
        .health
        .with_untracked(|h| h.session());
    let detail = match http {
        Some(status) => format!("{code} {status}"),
        None => code.to_owned(),
    };
    fail(ui, surface, session, kind, detail);
}

/// The full-screen player's stream failed (retries stopped).
pub fn failed(ui: &Ui) -> bool {
    ui.playing.get_untracked().is_some() && surface_failed(*ui, 0)
}

/// `surface`'s stream failed (retries stopped).
pub fn surface_failed(ui: Ui, surface: usize) -> bool {
    ui.player
        .stream(surface)
        .health
        .with_untracked(|h| h.failure().is_some())
}

/// The viewer's Retry: starts over with a fresh attempt count.
pub fn retry(ui: Ui) {
    retry_surface(ui, 0);
}

/// Retry on `surface`.
pub fn retry_surface(ui: Ui, surface: usize) {
    if target(ui, surface).is_none() {
        return;
    }
    let now = ui.now_ms();
    let mut session = 0;
    ui.player
        .stream(surface)
        .health
        .update(|h| session = h.retry_now(now));
    open(ui, surface, session);
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
    if ui.mode.get_untracked() == ViewMode::Fullscreen
        && crate::settings::flag(iptv_settings::keys::INFO_ON_SWITCH)
    {
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
    crate::lists::show_channel(ui, channel);
}

/// The last channel's file of playlist `source` (the shared one of older
/// builds for none).
fn last_channel_path(source: Option<SourceId>) -> std::path::PathBuf {
    crate::data::data_dir().join(match source {
        Some(id) => format!("{LAST_CHANNEL_FILE}-{}", id.0),
        None => LAST_CHANNEL_FILE.to_owned(),
    })
}

fn save_last_channel(source: Option<SourceId>, key: &str) {
    let path = last_channel_path(source);
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

fn load_last_channel(source: Option<SourceId>) -> Option<String> {
    let read = |path| {
        let key = std::fs::read_to_string(path).ok()?;
        let key = key.trim();
        (!key.is_empty()).then(|| key.to_owned())
    };
    read(last_channel_path(source)).or_else(|| read(last_channel_path(None)))
}

/// Playlist `source` was deleted: its last channel goes too.
pub fn forget_source(source: SourceId) {
    let _ = std::fs::remove_file(last_channel_path(Some(source)));
}

/// Another playlist is about to be shown (`data.rs`): what plays and the
/// channel history belong to the old one (channel keys are unique only
/// within a playlist, so the new one may reuse the playing key).
pub fn source_changed(ui: Ui) {
    crate::multiview::close(ui);
    if ui.playing.get_untracked().is_some() {
        log::info!("iptv player: another playlist is shown; stopping");
        ui.playing.set(None);
        stop(ui);
        if ui.mode.get_untracked() != ViewMode::Guide {
            ui.mode.set(ViewMode::Guide);
        }
    }
    ui.player.main().key.set(None);
    ui.player.history.set(ChannelHistory::default());
}

/// Data changes keep the playing channel (found again by key); stall
/// checks pause while paused.
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
            ui.player
                .main()
                .health
                .update(|h| h.set_paused(paused, now));
            if !paused {
                watch_stall(ui, 0);
            }
        });
    });
}

fn follow_playing(ui: Ui) {
    if crate::multiview::follow(ui) {
        return; // multiview follows its tiles, surface 0's too
    }
    let Some(playing) = ui.playing.get_untracked() else {
        return;
    };
    let key = ui.player.main().key.get_untracked();
    match key.and_then(|k| crate::schedule().find_channel(&k)) {
        // The guide keeps its own focus by identity (`lists::restore`).
        Some(row) => {
            if row != playing {
                ui.playing.set(Some(row));
            }
        }
        // Another source (or the channel left it): its row now means
        // another channel, so nothing plays.
        None => {
            log::info!("iptv player: the playing channel is not in the new guide; stopping");
            ui.playing.set(None);
            ui.player.main().key.set(None);
            stop(ui);
            if ui.mode.get_untracked() != ViewMode::Guide {
                ui.mode.set(ViewMode::Guide);
            }
        }
    }
}

/// `data.rs` put an imported source's schedule on screen. After the
/// startup refresh of a stored source (`refresh`), and with no wizard open,
/// the last channel starts once (`general.resume_last_channel`), unless
/// something already plays.
pub fn source_ready(ui: Ui, refresh: bool) {
    if !refresh
        || !crate::settings::flag(iptv_settings::keys::RESUME_LAST_CHANNEL)
        || !ui.player.resume.get_untracked()
        || ui.playing.get_untracked().is_some()
        || ui.wizard.with_untracked(Option::is_some)
    {
        return;
    }
    let last = load_last_channel(ui.library.shown.get_untracked());
    let Some(channel) = last.and_then(|k| crate::schedule().find_channel(&k)) else {
        return;
    };
    log::info!("iptv player: resuming the last channel");
    show_channel(ui, channel);
    ui.play(channel, Playback::Live);
    ui.mode.set(ViewMode::Fullscreen);
    ui.update_panels(|p, now| p.show_info(now));
}
