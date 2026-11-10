//! The player's words for waiting and failures: short, no addresses.

use iptv_core::playback::{Cause, ErrorKind, Waiting};

/// Our own wording for a failure.
pub(crate) fn failure_message(kind: &ErrorKind, catch_up: bool) -> String {
    let fixed = match kind {
        ErrorKind::Unavailable if catch_up => "This programme can't be replayed.",
        ErrorKind::Unavailable => "This channel has no stream address.",
        ErrorKind::Account => "The provider refused this account.",
        ErrorKind::Http(404 | 410) => "The provider doesn't have this stream.",
        _ if kind.cause() == Cause::Refused => return refused_message(),
        ErrorKind::Http(_) => "The provider answered with an error.",
        ErrorKind::Network => "Can't reach the provider. Check the connection.",
        ErrorKind::Format => "This stream's format isn't supported.",
        ErrorKind::Ended => "The stream ended.",
        ErrorKind::Stalled => "The stream stopped sending video.",
        ErrorKind::Resolve => "The portal didn't give a stream address.",
        ErrorKind::Other => "Playback stopped with an error.",
    };
    fixed.to_owned()
}

/// The provider refuses the stream for now: most often every stream the
/// account allows plays elsewhere.
fn refused_message() -> String {
    match crate::multiview::account_limit() {
        Some(1) => "Your provider allows 1 stream at once — another device may be using it.".into(),
        Some(n) => {
            format!("Your provider allows {n} streams at once — another device may be using one.")
        }
        None => "The provider is refusing this stream for now.".into(),
    }
}

/// The waiting pill's words.
pub(crate) fn waiting_text(waiting: &Waiting) -> String {
    match waiting {
        Waiting::Loading => "Loading…".into(),
        Waiting::Buffering => "Buffering…".into(),
        Waiting::Reconnecting { .. } => "Reconnecting…".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_accounts_streams_when_known() {
        crate::multiview::set_account_streams(None);
        let refused = failure_message(&ErrorKind::Http(509), false);
        assert_eq!(refused, "The provider is refusing this stream for now.");
        crate::multiview::set_account_streams(Some(2));
        let full = failure_message(&ErrorKind::Http(403), false);
        assert!(
            full.starts_with("Your provider allows 2 streams at once"),
            "{full}"
        );
        assert_eq!(
            failure_message(&ErrorKind::Http(404), false),
            "The provider doesn't have this stream."
        );
        let network = failure_message(&ErrorKind::Network, false);
        assert!(!network.contains("://"), "no addresses");
        assert_eq!(
            waiting_text(&Waiting::Reconnecting { attempt: 9 }),
            "Reconnecting…"
        );
        crate::multiview::set_account_streams(None);
    }
}
