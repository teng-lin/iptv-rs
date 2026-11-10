//! Errors returned by the provider clients.
//!
//! Messages never contain credentials, tokens, MAC addresses or response
//! bodies; a portal's own explanation (Stalker `msg`/`block_msg`) is kept
//! separately in [`StalkerError::portal_text`] so the UI can quote it.

use std::fmt;

/// Why a provider request or response could not be turned into a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderError {
    /// The source configuration is unusable (bad URL, blank credentials).
    InvalidConfig(String),
    /// The server answered with a non-success HTTP status.
    HttpStatus(u16),
    /// The body is not what the endpoint returns (not JSON, wrong shape,
    /// empty). The text describes the shape, never the body itself.
    InvalidBody(String),
    /// An Xtream panel answered a catalog request with an account object
    /// (for example `user_info.auth = 0`) instead of a list.
    AccountRejected,
    /// A Stalker/Ministra portal refused the request or the session.
    Stalker(StalkerError),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(why) => write!(f, "invalid source configuration: {why}"),
            Self::HttpStatus(code) => write!(f, "HTTP status {code}"),
            Self::InvalidBody(why) => write!(f, "unexpected response: {why}"),
            Self::AccountRejected => f.write_str("the provider rejected the account"),
            Self::Stalker(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<StalkerError> for ProviderError {
    fn from(err: StalkerError) -> Self {
        Self::Stalker(err)
    }
}

/// What kind of refusal a Stalker portal produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StalkerErrorKind {
    /// Token missing, stale or replaced (`Authorization failed.` and the JSON
    /// envelopes some panels use instead).
    AuthFailed,
    /// `get_profile` status 1 or `Access denied.`: blocked/disabled account,
    /// malformed MAC.
    Blocked,
    /// `get_profile` status 1 whose message names a device-id binding
    /// conflict. The remedy is restoring the first device id.
    DeviceConflict,
    /// `get_profile` status 2 and no login/password configured.
    LoginRequired,
    /// `do_auth` answered `{js: false}`.
    LoginRejected,
    /// The handshake answered without a token.
    NoToken,
    /// An API request was built before the session was authorized.
    NotAuthorized,
    /// Repeated authorization failures right after a fresh login.
    AuthLoop,
    /// `create_link` or another call answered `js.error`.
    Portal,
}

/// A Stalker refusal plus the portal's own explanation, if it gave one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StalkerError {
    pub kind: StalkerErrorKind,
    /// Markup-stripped `msg`/`block_msg`, or the plain-text failure body.
    pub portal_text: Option<String>,
}

impl StalkerError {
    pub fn new(kind: StalkerErrorKind) -> Self {
        Self {
            kind,
            portal_text: None,
        }
    }

    pub fn with_text(kind: StalkerErrorKind, text: Option<String>) -> Self {
        Self {
            kind,
            portal_text: text.filter(|t| !t.is_empty()),
        }
    }
}

impl fmt::Display for StalkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            StalkerErrorKind::AuthFailed => "portal authorization failed",
            StalkerErrorKind::Blocked => "portal refused the device",
            StalkerErrorKind::DeviceConflict => "portal reports a device id conflict",
            StalkerErrorKind::LoginRequired => "portal requires a login and password",
            StalkerErrorKind::LoginRejected => "portal rejected the login",
            StalkerErrorKind::NoToken => "portal handshake returned no token",
            StalkerErrorKind::NotAuthorized => "portal session is not authorized yet",
            StalkerErrorKind::AuthLoop => "portal keeps rejecting fresh sessions",
            StalkerErrorKind::Portal => "portal returned an error",
        };
        match &self.portal_text {
            Some(text) => write!(f, "{what}: {text}"),
            None => f.write_str(what),
        }
    }
}

impl std::error::Error for StalkerError {}
