//! Self-test for the hermetic fixture site (WS14.S1).
//!
//! The driver must serve exactly what it is told, follow redirects hop by hop
//! with a hop limit, record every request in arrival order (404s included), and
//! ignore fragments. The standard site must carry the links and rules the live
//! fixture (`deploy/compose/crawl-fixture/`) serves. Nothing here touches the
//! network.

mod support;

use quarry_runtime::driver::Driver;
use support::fixture_site::{
    FixtureReply, FixtureSiteDriver, FIXTURE_A, FIXTURE_A_DOCS, FIXTURE_B, HOST_B, MAX_REDIRECTS,
};
use url::Url;

fn url(raw: &str) -> Url {
    Url::parse(raw).expect("test url parses")
}

#[tokio::test]
async fn serves_configured_page_with_its_status_and_body() {
    let site = FixtureSiteDriver::new().serve(
        "http://fixture-a.crawl.test/docs/a",
        FixtureReply::html("<p>hello</p>"),
    );

    let response = site
        .fetch(&url("http://fixture-a.crawl.test/docs/a"))
        .await
        .expect("fetch");

    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"<p>hello</p>".to_vec());
    assert_eq!(
        response.final_url.as_str(),
        "http://fixture-a.crawl.test/docs/a"
    );
    assert!(response
        .headers
        .iter()
        .any(|(name, value)| name == "content-type" && value.starts_with("text/html")));
}

#[tokio::test]
async fn unconfigured_url_answers_404_and_is_still_recorded() {
    let site = FixtureSiteDriver::new();

    let response = site
        .fetch(&url("http://fixture-b.crawl.test/nothing-here"))
        .await
        .expect("fetch");

    assert_eq!(response.status, 404);
    let requests = site.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].status, 404);
    assert!(site.was_requested("http://fixture-b.crawl.test/nothing-here"));
}

#[tokio::test]
async fn fragment_is_ignored_for_lookup_and_for_the_record() {
    let site = FixtureSiteDriver::new().serve(
        "http://fixture-a.crawl.test/docs/a",
        FixtureReply::html("ok"),
    );

    let response = site
        .fetch(&url("http://fixture-a.crawl.test/docs/a#section-2"))
        .await
        .expect("fetch");

    assert_eq!(response.status, 200);
    let requests = site.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.fragment(), None);
    assert_eq!(
        requests[0].url.as_str(),
        "http://fixture-a.crawl.test/docs/a"
    );
}

#[tokio::test]
async fn robots_status_override_is_served_as_configured() {
    let site = FixtureSiteDriver::new().serve(
        "http://fixture-a.crawl.test/robots.txt",
        FixtureReply::text_with_status(503, "try later"),
    );

    let response = site
        .fetch(&url("http://fixture-a.crawl.test/robots.txt"))
        .await
        .expect("fetch");

    assert_eq!(response.status, 503);
    assert_eq!(response.body, b"try later".to_vec());
}

#[tokio::test]
async fn redirects_are_followed_and_every_hop_is_recorded() {
    let site = FixtureSiteDriver::new()
        .serve(
            "http://fixture-a.crawl.test/old",
            FixtureReply::redirect(301, "/docs/a"),
        )
        .serve(
            "http://fixture-a.crawl.test/docs/a",
            FixtureReply::html("moved here"),
        );

    let response = site
        .fetch(&url("http://fixture-a.crawl.test/old"))
        .await
        .expect("fetch");

    assert_eq!(response.status, 200);
    assert_eq!(
        response.final_url.as_str(),
        "http://fixture-a.crawl.test/docs/a"
    );
    let urls: Vec<String> = site
        .requests()
        .iter()
        .map(|request| request.url.to_string())
        .collect();
    assert_eq!(
        urls,
        vec![
            "http://fixture-a.crawl.test/old".to_owned(),
            "http://fixture-a.crawl.test/docs/a".to_owned(),
        ]
    );
}

#[tokio::test]
async fn cross_host_redirect_is_followed_and_recorded_on_the_other_host() {
    let site = FixtureSiteDriver::new()
        .serve(
            "http://fixture-a.crawl.test/go",
            FixtureReply::redirect(302, "http://fixture-b.crawl.test/ext"),
        )
        .serve(
            "http://fixture-b.crawl.test/ext",
            FixtureReply::html("external"),
        );

    let response = site
        .fetch(&url("http://fixture-a.crawl.test/go"))
        .await
        .expect("fetch");

    assert_eq!(response.status, 200);
    assert_eq!(response.final_url.host_str(), Some(HOST_B));
    assert_eq!(
        site.requested_urls_on(HOST_B),
        vec!["http://fixture-b.crawl.test/ext".to_owned()]
    );
}

#[tokio::test]
async fn redirect_loop_stops_after_the_hop_limit_and_returns_the_last_redirect() {
    let site = FixtureSiteDriver::new()
        .serve(
            "http://fixture-a.crawl.test/loop-a",
            FixtureReply::redirect(302, "/loop-b"),
        )
        .serve(
            "http://fixture-a.crawl.test/loop-b",
            FixtureReply::redirect(302, "/loop-a"),
        );

    let response = site
        .fetch(&url("http://fixture-a.crawl.test/loop-a"))
        .await
        .expect("fetch");

    assert_eq!(response.status, 302);
    // The first request plus one per followed hop.
    assert_eq!(site.requests().len(), MAX_REDIRECTS + 1);
}

#[tokio::test]
async fn requests_are_recorded_in_arrival_order_with_non_decreasing_instants() {
    let site = FixtureSiteDriver::standard_site();

    site.fetch(&url("http://fixture-a.crawl.test/docs/a"))
        .await
        .expect("fetch docs");
    site.fetch(&url("http://fixture-a.crawl.test/blog/b"))
        .await
        .expect("fetch blog");

    let requests = site.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url.path(), "/docs/a");
    assert_eq!(requests[1].url.path(), "/blog/b");
    assert!(requests[1].at >= requests[0].at);
}

#[tokio::test]
async fn standard_site_index_carries_the_documented_links() {
    let site = FixtureSiteDriver::standard_site();

    let response = site
        .fetch(&url(&format!("{FIXTURE_A}/")))
        .await
        .expect("fetch index");
    let body = String::from_utf8(response.body).expect("utf-8 index");

    assert_eq!(response.status, 200);
    for href in [
        "href=\"/docs/a\"",
        "href=\"/blog/b\"",
        "href=\"/private/secret\"",
        "src=\"/img/logo.png\"",
        "href=\"mailto:",
        "href=\"http://fixture-b.crawl.test/ext\"",
        "href=\"http://docs.fixture-a.crawl.test/\"",
    ] {
        assert!(body.contains(href), "index is missing {href}");
    }
}

#[tokio::test]
async fn standard_site_robots_has_the_disallow_and_crawl_delay_rules() {
    let site = FixtureSiteDriver::standard_site();

    let response = site
        .fetch(&url(&format!("{FIXTURE_A}/robots.txt")))
        .await
        .expect("fetch robots");
    let body = String::from_utf8(response.body).expect("utf-8 robots");

    assert_eq!(response.status, 200);
    assert!(body.contains("User-agent: *"));
    assert!(body.contains("Disallow: /private/"));
    assert!(body.contains("Crawl-delay: 2"));
}

#[tokio::test]
async fn standard_site_second_host_name_serves_the_same_index() {
    let site = FixtureSiteDriver::standard_site();

    let response = site
        .fetch(&url(&format!("{FIXTURE_A_DOCS}/")))
        .await
        .expect("fetch docs host");

    assert_eq!(response.status, 200);
    assert_eq!(
        site.requested_urls_on("docs.fixture-a.crawl.test"),
        vec!["http://docs.fixture-a.crawl.test/".to_owned()]
    );
}

#[tokio::test]
async fn standard_site_has_no_robots_on_the_external_host() {
    let site = FixtureSiteDriver::standard_site();

    let robots = site
        .fetch(&url(&format!("{FIXTURE_B}/robots.txt")))
        .await
        .expect("fetch");
    let ext = site
        .fetch(&url(&format!("{FIXTURE_B}/ext")))
        .await
        .expect("fetch ext");

    assert_eq!(robots.status, 404);
    assert_eq!(ext.status, 200);
    assert!(site.was_requested(&format!("{FIXTURE_B}/robots.txt")));
}
