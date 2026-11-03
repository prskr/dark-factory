//! Which addresses an untrusted outbound target may reach (`lifecycle-events` design D4).
//!
//! Project subscriptions are entered by users, so the control plane must not send them to its own
//! network: the delivery client's resolver drops every address this module refuses, and literal
//! IP URLs are checked when a subscription is saved and before each attempt.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::host::strip_brackets;

/// Whether `addr` is a public unicast address: not loopback, private (RFC 1918), CGNAT
/// (`100.64.0.0/10`), link-local, unique-local (`fc00::/7`), unspecified, broadcast or
/// documentation, and not an IPv4-mapped IPv6 form of one of these.
pub fn is_public(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map_or_else(|| is_public_v6(v6), is_public_v4),
    }
}

/// Whether the host of `url` may be connected to without resolving it: `true` for a domain name
/// (the resolver checks it), [`is_public`] for an IP literal, `false` without a host.
pub fn literal_host_is_public(url: &url::Url) -> bool {
    match url.host_str() {
        None => false,
        Some(host) => strip_brackets(host)
            .parse::<IpAddr>()
            .map_or(true, is_public),
    }
}

fn is_public_v4(addr: Ipv4Addr) -> bool {
    let [first, second, ..] = addr.octets();
    let cgnat = first == 100 && (second & 0xc0) == 64;
    !(addr.is_loopback()
        || addr.is_private()
        || cgnat
        || addr.is_link_local()
        || addr.is_unspecified()
        || addr.is_broadcast()
        || addr.is_documentation())
}

fn is_public_v6(addr: Ipv6Addr) -> bool {
    let [first, second, ..] = addr.segments();
    let documentation = first == 0x2001 && second == 0x0db8;
    !(addr.is_loopback()
        || addr.is_unspecified()
        || addr.is_unique_local()
        || addr.is_unicast_link_local()
        || documentation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(addr: &str) -> bool {
        is_public(addr.parse().unwrap())
    }

    #[test]
    fn internal_addresses_are_refused() {
        for addr in [
            "127.0.0.1",
            "127.255.0.9",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "100.127.255.254",
            "169.254.169.254",
            "0.0.0.0",
            "255.255.255.255",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:100.64.0.1",
        ] {
            assert!(!public(addr), "{addr} must be refused");
        }
    }

    #[test]
    fn public_addresses_pass() {
        for addr in [
            "1.1.1.1",
            "8.8.8.8",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001::200e",
            "::ffff:1.1.1.1",
        ] {
            assert!(public(addr), "{addr} must pass");
        }
    }

    #[test]
    fn literal_ip_urls_are_checked() {
        let check = |url: &str| literal_host_is_public(&url::Url::parse(url).unwrap());
        assert!(!check("http://127.0.0.1:8080/hook"));
        assert!(!check("https://[::1]/hook"));
        assert!(!check("https://[::ffff:10.0.0.1]/hook"));
        assert!(!check("http://169.254.169.254/latest/meta-data"));
        assert!(check("https://1.1.1.1/hook"));
        assert!(check("https://[2606:4700:4700::1111]/hook"));
        assert!(check("https://hooks.example.org/hook"));
    }
}
