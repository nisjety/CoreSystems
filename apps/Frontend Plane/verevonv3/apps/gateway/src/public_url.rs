use std::net::IpAddr;

use url::Url;

pub(crate) fn normalize_public_http_url(input: &str) -> std::result::Result<String, String> {
    let url = Url::parse(input.trim()).map_err(|_| "The URL is not valid.".to_owned())?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err("The URL must start with http:// or https://.".into()),
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with embedded credentials are not allowed.".into());
    }

    let host = url
        .host_str()
        .ok_or_else(|| "The URL is missing a hostname.".to_owned())?;
    if is_blocked_hostname(host) {
        return Err("Private or loopback URLs are not allowed.".into());
    }

    let ip_host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = ip_host.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err("Private or loopback URLs are not allowed.".into());
        }
    }

    Ok(url.to_string())
}

fn is_blocked_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        // `.internal` is reserved for internal use and is the GCP metadata vector
        // (metadata.google.internal); block it alongside loopback/mDNS names.
        || host.ends_with(".internal")
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 224
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                || (ip.octets()[0] == 198 && (18..=19).contains(&ip.octets()[1])))
        }
        IpAddr::V6(ip) => {
            // IPv4-mapped IPv6 (::ffff:a.b.c.d) must be judged by the IPv4 rules, or
            // an attacker tunnels a private/loopback/metadata v4 address past the v6 arm.
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast()
                || matches!(ip.segments()[0], 0x2001) && matches!(ip.segments()[1], 0x0db8))
        }
    }
}
