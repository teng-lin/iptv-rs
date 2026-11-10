//! The reqwest-backed client.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use iptv_import::{Fetch, FetchError, FetchErrorKind, StreamingResponse};
use iptv_providers::{HttpRequest, HttpResponse, Method};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use tokio::io::AsyncReadExt;

use crate::body::{Inner, NetBody};
use crate::error::from_reqwest;
use crate::file;
use crate::redirect::{self, Prepared};

/// Transport settings.
#[derive(Clone, PartialEq, Eq)]
pub struct NetConfig {
    pub connect_timeout: Duration,
    /// Maximum silence while waiting for response headers or body bytes.
    pub read_timeout: Duration,
    pub max_redirects: usize,
    /// Sent when a request has no `User-Agent` of its own. Providers that
    /// filter on it get theirs per source (M3U setting, Xtream/Stalker
    /// clients).
    pub user_agent: String,
    /// Buffered [`NetClient::fetch`] fails with `TooLarge` beyond this.
    pub max_buffered_bytes: usize,
    /// DER certificates trusted in addition to the Mozilla roots
    /// (self-hosted servers, user CAs).
    pub extra_roots_der: Vec<Vec<u8>>,
    /// Read `file://` URLs and absolute paths from the filesystem.
    pub allow_files: bool,
    /// `http://host:port` proxy for every request. Environment proxies are
    /// ignored.
    pub proxy: Option<String>,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(30),
            max_redirects: 10,
            user_agent: iptv_providers::xtream::DEFAULT_USER_AGENT.to_owned(),
            max_buffered_bytes: 128 * 1024 * 1024,
            extra_roots_der: Vec::new(),
            allow_files: true,
            proxy: None,
        }
    }
}

impl std::fmt::Debug for NetConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetConfig")
            .field("connect_timeout", &self.connect_timeout)
            .field("read_timeout", &self.read_timeout)
            .field("max_redirects", &self.max_redirects)
            .field("user_agent", &self.user_agent)
            .field("max_buffered_bytes", &self.max_buffered_bytes)
            .field("extra_roots", &self.extra_roots_der.len())
            .field("allow_files", &self.allow_files)
            // A proxy URL may carry credentials.
            .field("proxy", &self.proxy.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Performs provider requests. Cheap to clone (shared connection pool).
#[derive(Clone)]
pub struct NetClient {
    client: reqwest::Client,
    config: Arc<NetConfig>,
}

impl std::fmt::Debug for NetClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl NetClient {
    pub fn new(config: NetConfig) -> Result<Self, FetchError> {
        let tls = crate::tls::client_config(&config.extra_roots_der)?;
        let mut builder = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.read_timeout)
            // Redirects are followed in `redirect::execute` (header hygiene).
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .gzip(true)
            .deflate(true)
            .no_proxy();
        if let Some(proxy) = &config.proxy {
            let proxy = reqwest::Proxy::all(proxy).map_err(|_| {
                FetchError::new(FetchErrorKind::InvalidRequest, "invalid proxy address")
            })?;
            builder = builder.proxy(proxy);
        }
        let client = builder.build().map_err(from_reqwest)?;
        Ok(Self {
            client,
            config: Arc::new(config),
        })
    }

    /// With [`NetConfig::default`].
    pub fn with_defaults() -> Result<Self, FetchError> {
        Self::new(NetConfig::default())
    }

    pub fn config(&self) -> &NetConfig {
        &self.config
    }

    /// Sends the request and reads the whole body (up to
    /// [`NetConfig::max_buffered_bytes`]).
    pub async fn fetch(&self, request: HttpRequest) -> Result<HttpResponse, FetchError> {
        let limit = self.config.max_buffered_bytes;
        let resp = self.fetch_stream(request).await?;
        if resp.content_length.is_some_and(|n| n > limit as u64) {
            return Err(too_large(limit));
        }
        let mut body = Vec::with_capacity(resp.content_length.unwrap_or(0).min(1 << 20) as usize);
        match resp.body.0 {
            Inner::Http(mut r) => {
                while let Some(chunk) = r.chunk().await.map_err(from_reqwest)? {
                    if body.len() + chunk.len() > limit {
                        return Err(too_large(limit));
                    }
                    body.extend_from_slice(&chunk);
                }
            }
            Inner::File(f) => {
                let mut limited = f.take(limit as u64 + 1);
                limited
                    .read_to_end(&mut body)
                    .await
                    .map_err(|e| file::io_error(&e))?;
                if body.len() > limit {
                    return Err(too_large(limit));
                }
            }
            Inner::Done => {}
        }
        Ok(HttpResponse::new(resp.status, body))
    }

    /// Sends the request; the body is read with
    /// [`iptv_import::BodyStream::next_chunk`].
    pub async fn fetch_stream(
        &self,
        request: HttpRequest,
    ) -> Result<StreamingResponse<NetBody>, FetchError> {
        if let Some(path) = file::local_path(&request.url) {
            if !self.config.allow_files {
                return Err(FetchError::new(
                    FetchErrorKind::Unsupported,
                    "local files are disabled",
                ));
            }
            let (f, len) = file::open(&path?).await?;
            log::debug!("read local file ({len:?} bytes)");
            return Ok(StreamingResponse {
                status: 200,
                content_length: len,
                body: NetBody(Inner::File(f)),
            });
        }
        let shown = request.redacted_url();
        let req = self.build(request)?;
        log::debug!("{} {shown}", req.method);
        let resp = redirect::execute(&self.client, req, self.config.max_redirects, &shown)
            .await
            .inspect_err(|e| log::debug!("{shown}: {e}"))?;
        let status = resp.status().as_u16();
        log::debug!("{shown}: HTTP {status}");
        Ok(StreamingResponse {
            status,
            content_length: resp.content_length(),
            body: NetBody(Inner::Http(resp)),
        })
    }

    fn build(&self, request: HttpRequest) -> Result<Prepared, FetchError> {
        let invalid = |what: &str| FetchError::new(FetchErrorKind::InvalidRequest, what);
        let url = reqwest::Url::parse(request.url.trim()).map_err(|_| invalid("invalid URL"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(FetchError::new(
                FetchErrorKind::Unsupported,
                format!("unsupported scheme {}", url.scheme()),
            ));
        }
        let mut headers = HeaderMap::new();
        for (name, value) in &request.headers {
            let name = HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| invalid("invalid header name"))?;
            // Values may be secrets: never quote them.
            let value =
                HeaderValue::from_str(value.trim()).map_err(|_| invalid("invalid header value"))?;
            headers.append(name, value);
        }
        if !headers.contains_key(USER_AGENT) {
            let ua = HeaderValue::from_str(&self.config.user_agent)
                .map_err(|_| invalid("invalid default User-Agent"))?;
            headers.insert(USER_AGENT, ua);
        }
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
        };
        Ok(Prepared {
            method,
            url,
            headers,
            body: request.body,
        })
    }
}

fn too_large(limit: usize) -> FetchError {
    FetchError::new(FetchErrorKind::TooLarge, format!("more than {limit} bytes"))
}

impl Fetch for NetClient {
    type Body = NetBody;

    fn fetch(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, FetchError>> {
        NetClient::fetch(self, request)
    }

    fn fetch_stream(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<StreamingResponse<NetBody>, FetchError>> {
        NetClient::fetch_stream(self, request)
    }
}
