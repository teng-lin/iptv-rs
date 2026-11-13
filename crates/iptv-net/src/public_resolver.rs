//! DNS policy for content-supplied URLs. Return the vetted socket addresses to
//! reqwest directly: validating then resolving again would allow DNS rebinding.
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{io, net::SocketAddr};

#[derive(Debug, Default)]
pub struct PublicResolver;

fn checked(addresses: Vec<SocketAddr>) -> io::Result<Addrs> {
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|a| !iptv_providers::content_url::public_ip(a.ip()))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "content host resolved to a non-public address",
        ));
    }
    Ok(Box::new(addresses.into_iter()))
}
impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addresses = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            checked(addresses).map_err(Into::into)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_dns_answers_are_checked_before_returning_a_pinned_set() {
        assert!(
            checked(vec![
                "8.8.8.8:0".parse().unwrap(),
                "127.0.0.1:0".parse().unwrap()
            ])
            .is_err()
        );
        assert!(checked(vec!["[::ffff:10.0.0.1]:0".parse().unwrap()]).is_err());
        let mut addresses = checked(vec!["8.8.8.8:0".parse().unwrap()]).unwrap();
        assert_eq!(addresses.next().unwrap(), "8.8.8.8:0".parse().unwrap());
        assert!(addresses.next().is_none());
    }
}
