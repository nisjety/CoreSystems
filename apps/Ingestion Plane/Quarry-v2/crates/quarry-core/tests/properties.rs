//! Property tests for the contracts and security primitives that live in
//! `quarry-core` (URLs, IDs), `quarry-transform` (fingerprinting), and
//! `quarry-security` (SSRF heuristics). No I/O; no external services.
//!
//! Run as `cargo test -p quarry-core --test properties`.

use proptest::prelude::*;
use std::net::{IpAddr, Ipv6Addr};

use quarry_core::ids::kinds::RunKind;
use quarry_security::heur;
use quarry_transform::fingerprint::{content_fingerprint, text_fingerprint};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Realistic-ish URL: scheme + host + optional port + path + query.
fn arb_url() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("https".to_string()), Just("http".to_string())],
        prop_oneof![
            Just("example.com".to_string()),
            Just("api.example.com".to_string()),
            Just("a.b.c.d.e.f.g.example".to_string()),
            Just("localhost".to_string()),
            Just("10.0.0.1".to_string()),
            Just("203.0.113.5".to_string()),
        ],
        proptest::option::of(0u16..65535u16),
        proptest::option::of(proptest::collection::vec("[a-zA-Z0-9_-]{1,8}", 0..6)),
        proptest::option::of(proptest::collection::vec("[a-zA-Z0-9_=]{1,12}", 0..4)),
    )
        .prop_map(|(scheme, host, port, path_segs, query_kvs)| {
            let mut s = format!("{scheme}://{host}");
            if let Some(p) = port {
                s.push_str(&format!(":{p}"));
            }
            if let Some(segs) = path_segs {
                if !segs.is_empty() {
                    s.push('/');
                    s.push_str(&segs.join("/"));
                }
            }
            if let Some(kvs) = query_kvs {
                if !kvs.is_empty() {
                    s.push('?');
                    s.push_str(&kvs.join("&"));
                }
            }
            s
        })
}

fn reserved_ipv4_strategies() -> BoxedStrategy<String> {
    prop_oneof![
        Just("0.0.0.0".to_string()),
        Just("0.1.2.3".to_string()),
        Just("10.0.0.1".to_string()),
        Just("127.0.0.1".to_string()),
        Just("127.255.255.254".to_string()),
        Just("169.254.169.254".to_string()),
        Just("172.16.0.1".to_string()),
        Just("192.0.0.8".to_string()),
        Just("192.0.2.1".to_string()),
        Just("198.18.0.1".to_string()),
        Just("198.51.100.1".to_string()),
        Just("203.0.113.1".to_string()),
        Just("224.0.0.1".to_string()),
        Just("240.0.0.1".to_string()),
        Just("255.255.255.255".to_string()),
        Just("100.64.0.1".to_string()),
    ]
    .boxed()
}

fn public_ipv4_strategies() -> BoxedStrategy<String> {
    prop_oneof![
        Just("8.8.8.8".to_string()),
        Just("1.1.1.1".to_string()),
        Just("93.184.216.34".to_string()),
        Just("9.9.9.9".to_string()),
        Just("208.67.222.222".to_string()),
    ]
    .boxed()
}

proptest! {
    // -----------------------------------------------------------------
    // 1. URL normalization: parse is idempotent and lossless
    // -----------------------------------------------------------------
    #[test]
    fn url_parse_is_idempotent(s in arb_url()) {
        let once = url::Url::parse(&s).expect("generator should emit valid URLs");
        let once_str = once.as_str().to_string();
        let twice = url::Url::parse(&once_str).expect("round-trip parse must succeed");
        prop_assert_eq!(once_str, twice.as_str().to_string());
    }

    #[test]
    fn url_scheme_is_http_or_https(s in arb_url()) {
        let u = url::Url::parse(&s).unwrap();
        prop_assert!(matches!(u.scheme(), "http" | "https"),
                     "unexpected scheme: {}", u.scheme());
    }

    // -----------------------------------------------------------------
    // 2. Content fingerprint: stable, sensitive, no whitespace bleed
    // -----------------------------------------------------------------
    #[test]
    fn content_fingerprint_is_stable(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let a = content_fingerprint(&bytes);
        let b = content_fingerprint(&bytes);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn content_fingerprint_changes_with_content(
        a in proptest::collection::vec(any::<u8>(), 1..64),
        b in proptest::collection::vec(any::<u8>(), 1..64),
    ) {
        if a != b {
            prop_assert_ne!(content_fingerprint(&a), content_fingerprint(&b));
        }
    }

    #[test]
    fn text_fingerprint_ignores_whitespace_and_case(s in "[a-zA-Z0-9 \\t\\n]{1,64}") {
        let baseline = text_fingerprint(&s);
        let normalised = s.split_whitespace().collect::<Vec<_>>().join("");
        let upper = s.to_uppercase();
        prop_assert_eq!(&baseline, &text_fingerprint(&normalised));
        prop_assert_eq!(&baseline, &text_fingerprint(&upper));
    }

    // -----------------------------------------------------------------
    // 3. SSRF heuristics: every reserved range is blocked, every
    //    public-range sample is allowed. Pure data; no DNS.
    // -----------------------------------------------------------------
    #[test]
    fn ssrf_blocks_every_reserved_ipv4(literal in reserved_ipv4_strategies()) {
        let addr: IpAddr = literal.parse().unwrap();
        prop_assert!(heur::resolve_guard(&[addr]).is_some(),
                     "reserved IPv4 was accepted: {literal}");
        let url: url::Url = format!("http://{literal}/").parse().unwrap();
        prop_assert!(heur::blocks_private_host(&url).is_some(),
                     "reserved IPv4 host was accepted: {literal}");
    }

    #[test]
    fn ssrf_allows_public_ipv4(literal in public_ipv4_strategies()) {
        let addr: IpAddr = literal.parse().unwrap();
        prop_assert!(heur::resolve_guard(&[addr]).is_none(),
                     "public IPv4 was blocked: {literal}");
        let url: url::Url = format!("http://{literal}/").parse().unwrap();
        prop_assert!(heur::blocks_private_host(&url).is_none(),
                     "public IPv4 host was blocked: {literal}");
    }

    #[test]
    fn ssrf_blocks_ipv6_reserved_ranges(
        o0 in 0u16..0xffffu16, o1 in 0u16..0xffffu16,
        o2 in 0u16..0xffffu16, o3 in 0u16..0xffffu16,
    ) {
        // Ipv6Addr::new takes 8 segments, each u16. A segment value
        // `0xfc00` puts 0xfc in octet[0] (the high byte) — that's the
        // top of the fc00::/7 ULA range. The proptest sweeps the
        // lower segments to cover the full prefix.
        let reserved: Vec<Ipv6Addr> = vec![
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),                       // ::1
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0),                       // ::
            Ipv6Addr::new(0xfc00, o0, o1, o2, o3, 0, 0, 1),               // ULA
            Ipv6Addr::new(0xfd00, o0, o1, o2, o3, 0, 0, 1),               // ULA
            Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1),                   // link-local
            Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0, 1),              // NAT64
            Ipv6Addr::new(0x0100, 0, 0, 0, 0, 0, 0, 1),                   // discard
            Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1),              // documentation
        ];
        for addr in &reserved {
            let ip = IpAddr::V6(*addr);
            prop_assert!(heur::resolve_guard(&[ip]).is_some(),
                         "reserved IPv6 was accepted: {addr}");
        }
    }

    #[test]
    fn ssrf_allows_public_ipv6(
        s0 in 0x2000u16..0x3fffu16,
        s1 in 0u16..0xffffu16,
        s2 in 0u16..0xffffu16,
        s3 in 0u16..0xffffu16,
    ) {
        let addr = Ipv6Addr::new(s0, s1, s2, s3, 0, 0, 0, 1);
        let ip = IpAddr::V6(addr);
        prop_assert!(heur::resolve_guard(&[ip]).is_none(),
                     "public IPv6 was blocked: {addr}");
    }

    // -----------------------------------------------------------------
    // 4. ID generation: every new RunId is unique and well-formed.
    // -----------------------------------------------------------------
    #[test]
    fn run_ids_are_unique_and_well_formed(_i in 0u32..64u32) {
        let a: RunKind = quarry_core::ids::Id::new();
        let s = a.to_string();
        let back: RunKind = s.parse().expect("printed id must parse");
        prop_assert!(s.starts_with("run_"), "bad prefix: {s}");
        prop_assert_eq!(&a, &back);
        let b: RunKind = quarry_core::ids::Id::new();
        prop_assert_ne!(&a, &b);
    }
}
