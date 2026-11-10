//! rustls client configuration: ring provider, webpki roots, extra roots.

use std::sync::Arc;

use iptv_import::{FetchError, FetchErrorKind};
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;

pub(crate) fn client_config(
    extra_roots_der: &[Vec<u8>],
) -> Result<rustls::ClientConfig, FetchError> {
    let invalid = |what: &str| FetchError::new(FetchErrorKind::InvalidRequest, what);
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for der in extra_roots_der {
        roots
            .add(CertificateDer::from(der.clone()))
            .map_err(|_| invalid("an extra root certificate is not valid DER"))?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| invalid("no usable TLS protocol versions"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    // reqwest is built without HTTP/2.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}
