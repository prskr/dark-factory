//! Name resolution of the proxy and the destinations it refuses.

use std::io;
use std::net::IpAddr;

use async_trait::async_trait;
use hickory_resolver::TokioResolver;
use hickory_resolver::config::{LookupIpStrategy, ResolverOpts};
use ipnet::IpNet;

/// Ranges that are never a valid destination: private, loopback, link-local, CGNAT, unique local,
/// unspecified, multicast and reserved.
const ALWAYS_BLOCKED: &[&str] = &[
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "224.0.0.0/3",
    // NAT64: a translator maps these to IPv4 targets (`64:ff9b::a00:1` reaches 10.0.0.1).
    "64:ff9b::/96",
    "64:ff9b:1::/48",
    "::/128",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

/// Resolves hostnames; the proxy never lets the destination do it.
#[async_trait]
pub trait Resolver: Send + Sync {
    /// All addresses of `host`.
    async fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>>;
}

/// Resolver of the system configuration (`/etc/resolv.conf`), with caching.
pub struct SystemResolver(TokioResolver);

impl SystemResolver {
    pub fn new() -> io::Result<Self> {
        let mut builder = TokioResolver::builder_tokio().map_err(io::Error::other)?;
        let options = resolver_options(builder.options_mut().clone());
        *builder.options_mut() = options;
        Ok(Self(builder.build().map_err(io::Error::other)?))
    }
}

#[async_trait]
impl Resolver for SystemResolver {
    async fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        let lookup = self.0.lookup_ip(host).await.map_err(io::Error::other)?;
        Ok(lookup.iter().collect())
    }
}

/// Looks up IPv4 first: hickory defaults to IPv6 first, which fails from clusters without IPv6
/// egress when the destination is dual-stack (the proxy connects to the first address).
fn resolver_options(mut system: ResolverOpts) -> ResolverOpts {
    system.ip_strategy = LookupIpStrategy::Ipv4thenIpv6;
    system
}

/// The set of refused destination ranges.
#[derive(Debug, Clone)]
pub struct BlockedRanges(Vec<IpNet>);

impl BlockedRanges {
    /// The built-in ranges plus `extra` (the cluster's pod and Service CIDRs), as CIDR strings.
    pub fn new<'a>(
        extra: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, ipnet::AddrParseError> {
        ALWAYS_BLOCKED
            .iter()
            .copied()
            .chain(extra)
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map(Self)
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        // `::ffff:10.0.0.5` reaches 10.0.0.5.
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
            v4 => v4,
        };
        self.0.iter().any(|net| net.contains(&ip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_special_ranges_are_blocked() {
        let blocked = BlockedRanges::new([]).unwrap();
        for ip in [
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "64:ff9b::a00:1",
            "64:ff9b:1::a00:1",
            "::ffff:10.0.0.5",
            "::ffff:127.0.0.1",
        ] {
            assert!(blocked.contains(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "1.1.1.1",
            "172.32.0.1",
            "100.128.0.1",
            "93.184.216.34",
            "2606:4700::1111",
        ] {
            assert!(!blocked.contains(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn lookups_prefer_ipv4() {
        // What `SystemResolver::new` builds its resolver with: the system's options, IPv4 first.
        let mut system = ResolverOpts::default();
        system.attempts = 7;
        let options = resolver_options(system);
        assert_eq!(options.ip_strategy, LookupIpStrategy::Ipv4thenIpv6);
        assert_eq!(options.attempts, 7, "the other system settings are kept");
    }

    #[test]
    fn configured_cidrs_extend_the_list() {
        let blocked = BlockedRanges::new(["203.0.113.0/24"]).unwrap();
        assert!(blocked.contains("203.0.113.9".parse().unwrap()));
        assert!(BlockedRanges::new(["nonsense"]).is_err());
    }
}
