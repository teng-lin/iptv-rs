//! Playback provenance and the explicit synthetic-development override.
pub(crate) fn policy(public_only: bool) -> iptv_net::playback::PlaybackPolicy {
    iptv_net::playback::PlaybackPolicy {
        public_only,
        // Release packages cannot enable private content destinations, even if
        // a fixture URL was accidentally supplied in the build environment.
        #[cfg(debug_assertions)]
        allow_private_fixture: cfg!(test)
            || cfg!(feature = "test-support")
            || crate::data::DEV_PLAYLIST.is_some(),
    }
}
