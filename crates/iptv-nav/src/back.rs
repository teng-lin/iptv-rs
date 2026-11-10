//! Long Back next to the platform's Back. Where Back is a key the app's
//! bindings hear (NSH on Android), the platform still goes back on its
//! release (`on_back_press`): after a Long Back acted, that one step back
//! must not happen. [`BackGuard`] remembers it, for one Back, for at most
//! [`BackGuard::TIMEOUT_MS`] (in case no release comes).

/// Swallows the platform's Back that follows a Long Back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackGuard {
    /// When a Long Back acted (ms on the caller's clock).
    armed_at: Option<u64>,
}

impl BackGuard {
    /// How long a Long Back waits for its release.
    pub const TIMEOUT_MS: u64 = 5_000;

    /// A Long Back acted: the next platform Back is swallowed.
    pub fn long_back(&mut self, now: u64) {
        self.armed_at = Some(now);
    }

    /// A new Back press starts: whatever an earlier one left is gone (its
    /// release already went back, or never came).
    pub fn pressed(&mut self) {
        self.armed_at = None;
    }

    /// Whether the next platform Back is swallowed (the app then handles
    /// Back even where it would let the platform leave).
    pub fn armed(&self) -> bool {
        self.armed_at.is_some()
    }

    /// The platform's Back: `true` when it is the release of a Long Back
    /// that acted (swallow it). Either way the guard is spent.
    pub fn take(&mut self, now: u64) -> bool {
        let swallow = self
            .armed_at
            .is_some_and(|at| now.saturating_sub(at) <= Self::TIMEOUT_MS);
        self.armed_at = None;
        swallow
    }

    /// The timeout passed without a release: disarms; whether it did.
    pub fn expire(&mut self, now: u64) -> bool {
        match self.armed_at {
            Some(at) if now.saturating_sub(at) >= Self::TIMEOUT_MS => {
                self.armed_at = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_back_after_a_long_back_is_swallowed() {
        let mut g = BackGuard::default();
        assert!(!g.take(0), "a plain Back goes back");
        g.long_back(1_000);
        assert!(g.armed());
        assert!(g.take(1_400), "its release does not go back");
        assert!(!g.armed());
        assert!(!g.take(1_500), "once");
    }

    #[test]
    fn a_new_press_or_the_timeout_disarms() {
        let mut g = BackGuard::default();
        g.long_back(0);
        g.pressed();
        assert!(!g.take(10), "a new press starts over");
        g.long_back(0);
        assert!(!g.expire(BackGuard::TIMEOUT_MS - 1));
        assert!(g.expire(BackGuard::TIMEOUT_MS));
        assert!(!g.take(BackGuard::TIMEOUT_MS + 1));
        g.long_back(0);
        assert!(
            !g.take(BackGuard::TIMEOUT_MS + 1),
            "too late to be its release"
        );
    }
}
