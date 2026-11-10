//! Xtream Codes import: account probes → live categories + streams, then
//! optional VOD and series lists.
//!
//! Live streams are required; live categories and the optional lists only
//! add warnings when they fail. An account object where a list was expected
//! is an authentication failure, never an empty catalog.

use iptv_providers::model::{ContentKind, XtreamCredentials};
use iptv_providers::xtream::{
    AccountInfo, AccountStatus, XtreamClient, parse_account_info, parse_categories,
    parse_live_streams, parse_series_list, parse_vod_streams,
};
use iptv_providers::{HttpRequest, HttpResponse, ProviderError, SecretUrl};

use crate::catalog::{Catalog, ImportOptions, SourceMeta};
use crate::ctx::Ctx;
use crate::error::{AuthProblem, ImportError, Step};
use crate::fetch::Fetch;
use crate::progress::{Phase, Progress, ProgressSink};

pub(crate) async fn import_xtream<F: Fetch, P: ProgressSink>(
    ctx: &mut Ctx<'_, F, P>,
    creds: &XtreamCredentials,
    opts: &ImportOptions,
) -> Result<Catalog, ImportError> {
    let client = XtreamClient::from_credentials(creds).map_err(|e| match e {
        ProviderError::InvalidConfig(why) => ImportError::InvalidConfig(why),
        other => ImportError::InvalidConfig(other.to_string()),
    })?;
    ctx.phase(Phase::Account);
    let account = account(ctx, &client, opts.now).await?;
    let mut catalog = Catalog::new(SourceMeta::Xtream {
        base_url: client.base_url().to_owned(),
        xmltv_url: SecretUrl::new(client.xmltv_url()),
        account: account.clone(),
    });
    if account.is_none() {
        catalog.warn(
            Step::AccountInfo,
            "the panel did not report account details",
        );
    }

    let live = ContentKind::Live;
    groups(ctx, &mut catalog, live, client.live_categories()).await?;
    ctx.phase(Phase::Items(live));
    let resp = list_response(ctx, Step::Items(live), client.live_streams(None)).await?;
    let streams =
        parse_live_streams(&resp).map_err(|e| ImportError::provider(Step::Items(live), e))?;
    drop(resp);
    catalog.channels = catalog.absorb(Step::Items(live), None, streams);
    let referenced: Vec<_> = catalog
        .channels
        .iter()
        .flat_map(|c| c.groups.clone())
        .collect();
    catalog.add_missing_groups(live, referenced.iter());
    items_done(ctx, live, catalog.channels.len());

    if opts.include_vod {
        let kind = ContentKind::Movie;
        let r = groups(ctx, &mut catalog, kind, client.vod_categories()).await;
        soften(&mut catalog, r)?;
        let r = async {
            let resp = list_response(ctx, Step::Items(kind), client.vod_streams(None)).await?;
            parse_vod_streams(&resp).map_err(|e| ImportError::provider(Step::Items(kind), e))
        }
        .await;
        if let Some(items) = soften(&mut catalog, r)? {
            catalog.vod = catalog.absorb(Step::Items(kind), None, items);
            let referenced: Vec<_> = catalog.vod.iter().flat_map(|v| v.groups.clone()).collect();
            catalog.add_missing_groups(kind, referenced.iter());
            items_done(ctx, kind, catalog.vod.len());
        }
    }
    if opts.include_series {
        let kind = ContentKind::Series;
        let r = groups(ctx, &mut catalog, kind, client.series_categories()).await;
        soften(&mut catalog, r)?;
        let r = async {
            let resp = list_response(ctx, Step::Items(kind), client.series(None)).await?;
            parse_series_list(&resp).map_err(|e| ImportError::provider(Step::Items(kind), e))
        }
        .await;
        if let Some(items) = soften(&mut catalog, r)? {
            catalog.series = catalog.absorb(Step::Items(kind), None, items);
            let referenced: Vec<_> = catalog
                .series
                .iter()
                .flat_map(|s| s.groups.clone())
                .collect();
            catalog.add_missing_groups(kind, referenced.iter());
            items_done(ctx, kind, catalog.series.len());
        }
    }

    if catalog.is_empty() {
        return Err(ImportError::Empty {
            step: Step::Items(live),
        });
    }
    catalog.set_transport_stats(&ctx.stats);
    ctx.phase(Phase::Done);
    Ok(catalog)
}

fn items_done<F: Fetch, P: ProgressSink>(ctx: &mut Ctx<'_, F, P>, kind: ContentKind, n: usize) {
    let mut p = Progress::phase(Phase::Items(kind));
    p.items = n as u64;
    ctx.report(p);
}

/// The result of an optional step: non-fatal failures become catalog warnings.
pub(crate) fn soften<T>(
    catalog: &mut Catalog,
    result: Result<T, ImportError>,
) -> Result<Option<T>, ImportError> {
    match result {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_fatal() => Err(e),
        Err(e) => {
            let at = e.step().unwrap_or(Step::AccountInfo);
            catalog.warn(at, e.to_string());
            Ok(None)
        }
    }
}

/// A list response that must be a success.
async fn list_response<F: Fetch, P: ProgressSink>(
    ctx: &mut Ctx<'_, F, P>,
    step: Step,
    req: HttpRequest,
) -> Result<HttpResponse, ImportError> {
    let resp = ctx.send(step, req).await?;
    if !resp.is_success() {
        return Err(ImportError::http(step, resp.status));
    }
    Ok(resp)
}

/// Categories of one kind. Failures other than network, auth and
/// cancellation are warnings: items still import (with placeholder groups).
async fn groups<F: Fetch, P: ProgressSink>(
    ctx: &mut Ctx<'_, F, P>,
    catalog: &mut Catalog,
    kind: ContentKind,
    req: HttpRequest,
) -> Result<(), ImportError> {
    let step = Step::Categories(kind);
    ctx.phase(Phase::Groups(kind));
    let parsed = match list_response(ctx, step, req).await {
        Ok(resp) => parse_categories(&resp, kind).map_err(|e| ImportError::provider(step, e)),
        Err(e) => Err(e),
    };
    match parsed {
        Ok(import) => {
            let items = catalog.absorb(step, None, import);
            catalog.groups.extend(items);
            Ok(())
        }
        Err(
            e @ (ImportError::Cancelled | ImportError::Network { .. } | ImportError::Auth { .. }),
        ) => Err(e),
        Err(e) => {
            catalog.warn(step, e.to_string());
            Ok(())
        }
    }
}

/// Probes the account endpoints in order. Inactive/expired accounts fail;
/// a panel that never reports `user_info` but answers is let through (the
/// catalog requests will tell).
async fn account<F: Fetch, P: ProgressSink>(
    ctx: &mut Ctx<'_, F, P>,
    client: &XtreamClient,
    now: i64,
) -> Result<Option<AccountInfo>, ImportError> {
    let step = Step::AccountInfo;
    let mut answered = false;
    let mut last: Option<ImportError> = None;
    for req in client.account_info_probes() {
        let resp = match ctx.send(step, req).await {
            Ok(r) => r,
            Err(ImportError::Network { step, error }) if !error.kind.is_host_failure() => {
                last = Some(ImportError::Network { step, error });
                continue;
            }
            Err(e) => return Err(e),
        };
        if !resp.is_success() {
            last = Some(ImportError::http(step, resp.status));
            continue;
        }
        answered = true;
        match parse_account_info(&resp, now) {
            Ok(info) => match info.status {
                AccountStatus::Active => return Ok(Some(info)),
                AccountStatus::Inactive | AccountStatus::Expired => {
                    return Err(ImportError::Auth {
                        step,
                        problem: AuthProblem::XtreamAccount {
                            status: info.status,
                            message: info.message,
                        },
                    });
                }
                AccountStatus::Unavailable => {}
            },
            Err(e) => last = Some(ImportError::provider(step, e)),
        }
    }
    if answered {
        return Ok(None);
    }
    Err(last.unwrap_or(ImportError::HttpStatus { step, status: 0 }))
}
