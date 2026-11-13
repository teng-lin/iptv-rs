//! Bounded failures are fatal even after usable rows have arrived.
use super::common::{FakeFetch, block_on};
use iptv_import::{
    CancelToken, ImportError, ImportLimits, ImportOptions, LimitKind, NoProgress, import_source,
};
use iptv_providers::{
    SecretUrl,
    model::{M3uSourceConfig, SourceConfig},
};

fn import(bytes: Vec<u8>, limits: ImportLimits) -> ImportError {
    let fetch = FakeFetch::body(bytes);
    let config = SourceConfig::M3u(M3uSourceConfig {
        location: SecretUrl::new("https://feed.example.test/list"),
        user_agent: None,
        referrer: None,
    });
    let mut options = ImportOptions::new(0);
    options.limits = limits;
    block_on(import_source(
        &fetch,
        &config,
        &options,
        &mut NoProgress,
        &CancelToken::new(),
    ))
    .unwrap_err()
}
#[test]
fn input_line_item_and_gzip_budgets_return_typed_failures() {
    let playlist = b"#EXTM3U\n#EXTINF:-1,One\nhttps://media.example.test/one\n#EXTINF:-1,Two\nhttps://media.example.test/two\n";
    for (limits, kind) in [
        (
            ImportLimits {
                input_bytes: 20,
                ..Default::default()
            },
            LimitKind::InputBytes,
        ),
        (
            ImportLimits {
                record_bytes: 20,
                ..Default::default()
            },
            LimitKind::RecordBytes,
        ),
        (
            ImportLimits {
                items: 1,
                ..Default::default()
            },
            LimitKind::Items,
        ),
        (
            ImportLimits {
                decoded_bytes: 20,
                ..Default::default()
            },
            LimitKind::DecodedBytes,
        ),
    ] {
        assert!(
            matches!(import(playlist.to_vec(), limits), ImportError::Limit { error, .. } if error.kind == kind)
        );
    }
    let expanded = vec![b'x'; 100_000];
    let compressed = super::common::synth::gzip(&expanded);
    assert!(
        matches!(import(compressed, ImportLimits { decoded_bytes: 1024, ..Default::default() }),
        ImportError::Limit { error, .. } if error.kind == LimitKind::DecodedBytes)
    );
}
