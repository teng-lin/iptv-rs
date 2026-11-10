//! EPG import: per source, a chunked download into [`XmltvParser`] (gzip
//! detected automatically) and a [`SourceBuilder`], then one [`GuideStore`]
//! with every source by priority, matched against the catalog's channels.
//!
//! A failing source does not discard the others: its error is kept in
//! [`EpgSourceOutcome`]. The import fails only when every source failed,
//! or on cancellation.

use iptv_epg::{
    ChannelKey, Compression, GuideStore, IngestReport, MatchIndex, MatchReport, Overrides,
    ParseOptions, ParseStats, PlaylistChannel, SourceBuilder, SourceId, StoreOptions, TimePolicy,
    UnixTime, XmltvParser,
};
use iptv_providers::m3u::M3uHeader;
use iptv_providers::model::Channel;
use iptv_providers::{HttpRequest, SecretUrl};

use crate::cancel::CancelToken;
use crate::ctx::Ctx;
use crate::error::{ImportError, ParseProblem, Step};
use crate::fetch::Fetch;
use crate::progress::{Phase, Progress, ProgressSink};

/// One XMLTV source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpgSourceConfig {
    pub id: SourceId,
    /// May carry credentials (Xtream `xmltv.php`).
    pub url: SecretUrl,
    /// Higher wins when several sources have a channel.
    pub priority: i32,
    pub user_agent: Option<String>,
    /// Offset for times without a zone, and a per-source shift.
    pub time: TimePolicy,
    pub store: StoreOptions,
}

impl EpgSourceConfig {
    pub fn new(id: SourceId, url: SecretUrl) -> Self {
        Self {
            id,
            url,
            priority: 0,
            user_agent: None,
            time: TimePolicy::UTC,
            store: StoreOptions::default(),
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// The playlist header's `url-tvg`/`x-tvg-url`/`tvg-url` entries, ids
    /// from `first_id`, the first URL with the highest priority.
    pub fn from_m3u_header(header: &M3uHeader, first_id: u32) -> Vec<Self> {
        let n = header.epg_urls.len() as i32;
        header
            .epg_urls
            .iter()
            .enumerate()
            .map(|(i, url)| {
                Self::new(SourceId(first_id + i as u32), SecretUrl::new(url.clone()))
                    .with_priority(n - i as i32)
            })
            .collect()
    }
}

/// Counters for one source that loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpgSourceStats {
    pub parse: ParseStats,
    pub ingest: IngestReport,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpgSourceOutcome {
    pub id: SourceId,
    pub result: Result<EpgSourceStats, ImportError>,
}

/// The loaded guide and how well it matched.
pub struct EpgImport {
    pub guide: GuideStore,
    pub matches: MatchReport,
    /// One entry per configured source, in input order.
    pub sources: Vec<EpgSourceOutcome>,
}

impl std::fmt::Debug for EpgImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpgImport")
            .field("sources", &self.sources)
            .field("matches", &self.matches)
            .field("bound", &self.guide.bound_count())
            .finish()
    }
}

/// Downloads and parses every source, then matches `channels` (tvg-id,
/// then names; `overrides` win). `now` drives retention.
pub async fn import_epg<F: Fetch, P: ProgressSink>(
    fetch: &F,
    sources: &[EpgSourceConfig],
    channels: &[Channel],
    overrides: &Overrides,
    now: UnixTime,
    progress: &mut P,
    cancel: &CancelToken,
) -> Result<EpgImport, ImportError> {
    let mut ctx = Ctx::new(fetch, progress, cancel);
    let mut guide = GuideStore::new();
    let mut index = MatchIndex::new();
    let mut outcomes = Vec::with_capacity(sources.len());
    for cfg in sources {
        ctx.check()?;
        let result = match load_source(&mut ctx, cfg, now).await {
            Ok((src, stats)) => {
                index.add_source(cfg.id, cfg.priority, src.channels());
                guide.replace_source(src);
                Ok(stats)
            }
            Err(ImportError::Cancelled) => return Err(ImportError::Cancelled),
            Err(e) => Err(e),
        };
        outcomes.push(EpgSourceOutcome { id: cfg.id, result });
    }
    if let Some(Err(first)) = outcomes.first().map(|o| &o.result)
        && outcomes.iter().all(|o| o.result.is_err())
    {
        return Err(first.clone());
    }

    ctx.phase(Phase::Matching);
    let keys: Vec<ChannelKey> = channels
        .iter()
        .map(|c| ChannelKey::new(c.id.as_str()))
        .collect();
    let playlist = channels.iter().zip(&keys).map(|(c, key)| PlaylistChannel {
        key,
        tvg_id: c.epg_id.as_deref(),
        name: &c.name,
        tvg_name: c.epg_name.as_deref(),
    });
    let result = index.match_channels(playlist, overrides);
    guide.set_matches(result.matches);
    let mut done = Progress::phase(Phase::Done);
    done.items = guide.bound_count() as u64;
    ctx.report(done);
    Ok(EpgImport {
        guide,
        matches: result.report,
        sources: outcomes,
    })
}

async fn load_source<F: Fetch, P: ProgressSink>(
    ctx: &mut Ctx<'_, F, P>,
    cfg: &EpgSourceConfig,
    now: UnixTime,
) -> Result<(iptv_epg::EpgSource, EpgSourceStats), ImportError> {
    let step = Step::Guide(cfg.id);
    let url = cfg.url.expose().trim();
    if url.is_empty() {
        return Err(ImportError::InvalidConfig("EPG address is required".into()));
    }
    let mut req = HttpRequest::get(url);
    if let Some(ua) = cfg
        .user_agent
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        req = req.with_header("User-Agent", ua);
    }
    let mut parser = XmltvParser::new(ParseOptions {
        time: cfg.time,
        compression: Compression::Auto,
        descriptions: cfg.store.store_descriptions,
        ..ParseOptions::default()
    });
    let mut builder = SourceBuilder::new(cfg.id, cfg.priority, cfg.store, now);
    let mut items = 0u64;
    let decode =
        |e: iptv_epg::DecodeError| ImportError::parse(step, ParseProblem::Decode(e.to_string()));
    ctx.phase(Phase::Guide(cfg.id));
    let bytes = ctx
        .stream(step, Phase::Guide(cfg.id), req, |chunk| {
            parser
                .feed(chunk, &mut |item| {
                    items += 1;
                    builder.push(item)
                })
                .map_err(decode)?;
            Ok(items)
        })
        .await?;
    let parse = parser
        .finish(&mut |item| builder.push(item))
        .map_err(decode)?;
    if parse.channels == 0 && parse.programmes == 0 {
        return Err(if bytes == 0 {
            ImportError::Empty { step }
        } else {
            ImportError::parse(step, ParseProblem::NotXmltv)
        });
    }
    let src = builder.finish();
    let stats = EpgSourceStats {
        parse,
        ingest: src.report(),
        bytes,
    };
    let mut p = Progress::phase(Phase::Guide(cfg.id));
    p.items = stats.parse.programmes;
    p.bytes = bytes;
    ctx.report(p);
    Ok((src, stats))
}
