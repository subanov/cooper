//! The client's IP address behind trusted reverse proxies.
//!
//! The TCP peer is the client unless it is a trusted proxy (`COOPER_TRUSTED_PROXIES`);
//! then the right-most `X-Forwarded-For` address that is not itself a trusted proxy is
//! the client. Untrusted peers cannot spoof their address with the header.

use std::net::IpAddr;
use std::str::FromStr;

use http::HeaderMap;

/// An IP network such as `10.0.0.0/8`, `::1/128` or a single address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpNetwork {
    addr: IpAddr,
    prefix: u8,
}

impl FromStr for IpNetwork {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (addr, prefix) = match s.trim().split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s.trim(), None),
        };
        let addr: IpAddr = addr
            .parse()
            .map_err(|_| format!("{s:?} is not an IP address or network"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => max,
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| format!("{s:?} has an invalid prefix"))?,
        };
        Ok(Self { addr, prefix })
    }
}

impl IpNetwork {
    /// Whether `ip` is inside this network.
    #[must_use]
    pub fn contains(&self, ip: IpAddr) -> bool {
        let (net, ip, bits) = match (self.addr, ip) {
            (IpAddr::V4(n), IpAddr::V4(i)) => {
                (u128::from(u32::from(n)), u128::from(u32::from(i)), 32)
            }
            (IpAddr::V6(n), IpAddr::V6(i)) => (u128::from(n), u128::from(i), 128),
            _ => return false,
        };
        let shift = bits - u32::from(self.prefix);
        if shift >= 128 {
            return true;
        }
        (net >> shift) == (ip >> shift)
    }
}

/// The client address of a request whose TCP peer is `peer`.
#[must_use]
pub fn client_ip(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted: &[IpNetwork],
) -> Option<IpAddr> {
    let peer = peer?;
    let is_trusted = |ip: IpAddr| trusted.iter().any(|n| n.contains(ip));
    if !is_trusted(peer) {
        return Some(peer);
    }
    let forwarded: Vec<IpAddr> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|a| a.trim().parse().ok())
        .collect();
    Some(
        forwarded
            .into_iter()
            .rev()
            .find(|ip| !is_trusted(*ip))
            .unwrap_or(peer),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    fn nets(specs: &[&str]) -> Vec<IpNetwork> {
        specs.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn networks_match_by_prefix() {
        let n: IpNetwork = "10.1.0.0/16".parse().unwrap();
        assert!(n.contains("10.1.200.3".parse().unwrap()));
        assert!(!n.contains("10.2.0.1".parse().unwrap()));
        assert!(
            "0.0.0.0/0"
                .parse::<IpNetwork>()
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
        assert!(
            "::1"
                .parse::<IpNetwork>()
                .unwrap()
                .contains("::1".parse().unwrap())
        );
        assert!(!n.contains("::1".parse().unwrap()));
        assert!("10.0.0.0/33".parse::<IpNetwork>().is_err());
        assert!("nope".parse::<IpNetwork>().is_err());
    }

    #[test]
    fn forwarded_for_is_only_believed_from_trusted_proxies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4, 203.0.113.7, 10.0.0.2"),
        );
        let trusted = nets(&["10.0.0.0/8"]);
        assert_eq!(
            client_ip(Some("10.0.0.1".parse().unwrap()), &headers, &trusted),
            Some("203.0.113.7".parse().unwrap())
        );
        assert_eq!(
            client_ip(Some("198.51.100.1".parse().unwrap()), &headers, &trusted),
            Some("198.51.100.1".parse().unwrap())
        );
        assert_eq!(
            client_ip(
                Some("10.0.0.1".parse().unwrap()),
                &HeaderMap::new(),
                &trusted
            ),
            Some("10.0.0.1".parse().unwrap())
        );
        assert_eq!(client_ip(None, &headers, &trusted), None);
    }
}
