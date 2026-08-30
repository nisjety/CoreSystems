//! Property tests for cache-key derivation in `quarry-edge`.
//!
//! These mirror the deterministic `#[test]` cases in
//! `src/cache.rs::fingerprint_tests` and `src/cache.rs::search_cache_tests`
//! but exercise a much wider input space via `proptest`. No I/O; no
//! external services. Runs as `cargo test -p quarry-edge --tests`.

use proptest::prelude::*;

use quarry_edge::cache::{fingerprint, SearchCache};

fn arb_url() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("https".to_string()), Just("http".to_string())],
        prop_oneof![
            Just("example.com".to_string()),
            Just("api.example.com".to_string()),
            Just("203.0.113.5".to_string()),
        ],
        proptest::option::of(0u16..65535u16),
        proptest::option::of(proptest::collection::vec("[a-zA-Z0-9_-]{1,8}", 0..4)),
    )
        .prop_map(|(scheme, host, port, path_segs)| {
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
            s
        })
}

fn arb_vary_header() -> impl Strategy<Value = (String, String)> {
    (
        prop_oneof![
            Just("Accept".to_string()),
            Just("Accept-Language".to_string()),
            Just("User-Agent".to_string()),
            Just("Authorization".to_string()),
        ],
        proptest::collection::vec(any::<u8>(), 1..16),
    )
        .prop_map(|(n, v)| (n, String::from_utf8_lossy(&v).into_owned()))
}

proptest! {
    #[test]
    fn cache_key_is_stable(url in arb_url(), h in arb_vary_header()) {
        let (n, v) = h;
        let a = fingerprint(&url, &[(n.as_str(), v.as_str())], false);
        let b = fingerprint(&url, &[(n.as_str(), v.as_str())], false);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn cache_key_is_header_order_independent(
        url in arb_url(),
        a in arb_vary_header(),
        b in arb_vary_header(),
    ) {
        let (an, av) = a;
        let (bn, bv) = b;
        let forward = fingerprint(
            &url,
            &[(an.as_str(), av.as_str()), (bn.as_str(), bv.as_str())],
            false,
        );
        let reverse = fingerprint(
            &url,
            &[(bn.as_str(), bv.as_str()), (an.as_str(), av.as_str())],
            false,
        );
        prop_assert_eq!(forward, reverse);
    }

    #[test]
    fn cache_key_changes_with_js_flag(url in arb_url()) {
        let without_js = fingerprint(&url, &[], false);
        let with_js = fingerprint(&url, &[], true);
        prop_assert_ne!(without_js, with_js);
    }

    #[test]
    fn cache_key_changes_with_url(a in arb_url(), b in arb_url()) {
        prop_assume!(a != b);
        prop_assert_ne!(fingerprint(&a, &[], false), fingerprint(&b, &[], false));
    }

    #[test]
    fn search_cache_key_is_org_scoped(
        org_a in "[a-z0-9_]{1,12}",
        org_b in "[a-z0-9_]{1,12}",
        query in "[a-z ]{1,32}",
        params in "[a-z0-9=|&]{1,32}",
    ) {
        prop_assume!(org_a != org_b);
        let a = SearchCache::key(&org_a, &query, &params);
        let b = SearchCache::key(&org_b, &query, &params);
        prop_assert_ne!(&a, &b);
        let prefix_a = format!("quarry:search:{org_a}:");
        let prefix_b = format!("quarry:search:{org_b}:");
        prop_assert!(a.starts_with(&prefix_a), "a={a} should start with {prefix_a}");
        prop_assert!(b.starts_with(&prefix_b), "b={b} should start with {prefix_b}");
    }

    #[test]
    fn search_cache_key_changes_on_query(
        org in "[a-z0-9_]{1,12}",
        q1 in "[a-z ]{1,16}",
        q2 in "[a-z ]{1,16}",
        params in "[a-z0-9=|&]{1,16}",
    ) {
        prop_assume!(q1 != q2);
        prop_assert_ne!(
            SearchCache::key(&org, &q1, &params),
            SearchCache::key(&org, &q2, &params),
        );
    }
}
