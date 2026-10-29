use url::Url;

pub fn normalize_query(input: &str) -> Option<String> {
    let collapsed = input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

pub fn normalize_host(input: &str) -> Option<String> {
    let parsed_host = Url::parse(input)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned));

    let candidate = parsed_host.unwrap_or_else(|| input.to_string());
    let host = candidate.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim_start_matches("www.").to_string();

    if host.is_empty() || host.contains('/') || host.contains(' ') {
        None
    } else {
        Some(host)
    }
}

pub fn bucket_for_org(org_id: &str) -> String {
    let hash = blake3::hash(org_id.as_bytes());
    format!("org_{}", &hash.to_hex()[..16])
}

pub fn stable_object(prefix: &str, text: &str) -> String {
    let hash = blake3::hash(text.as_bytes());
    format!("{}:{}", prefix, &hash.to_hex()[..24])
}

pub fn bounded_limit(limit: Option<usize>, default: usize, max: usize) -> usize {
    limit.unwrap_or(default).clamp(1, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_normalization_collapses_whitespace() {
        assert_eq!(
            normalize_query("  Find   me   spa  ").as_deref(),
            Some("Find me spa")
        );
        assert_eq!(normalize_query("  "), None);
    }

    #[test]
    fn host_normalization_accepts_urls_and_hosts() {
        assert_eq!(
            normalize_host("https://www.Example.com/path").as_deref(),
            Some("example.com")
        );
        assert_eq!(normalize_host("WWW.ACME.NO.").as_deref(), Some("acme.no"));
        assert_eq!(normalize_host("not a host"), None);
    }

    #[test]
    fn org_buckets_are_stable_and_non_raw() {
        let a = bucket_for_org("org_123");
        let b = bucket_for_org("org_123");
        assert_eq!(a, b);
        assert_ne!(a, "org_123");
        assert!(a.starts_with("org_"));
    }
}
