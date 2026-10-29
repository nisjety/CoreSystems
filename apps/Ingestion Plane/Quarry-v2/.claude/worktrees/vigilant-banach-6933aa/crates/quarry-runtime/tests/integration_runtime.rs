use quarry_runtime::{Driver, StaticDriver};
use std::time::Duration;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn static_driver_fetches_from_wiremock() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_string("<html>hi</html>"),
        )
        .mount(&server)
        .await;

    let driver = StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap();
    let url = Url::parse(&server.uri()).unwrap();
    let resp = driver.fetch(&url).await.unwrap();

    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"<html>hi</html>");
    assert!(resp
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type")));
}

#[tokio::test]
async fn static_driver_returns_500_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let driver = StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap();
    let url = Url::parse(&server.uri()).unwrap();
    let resp = driver.fetch(&url).await.unwrap();

    assert_eq!(resp.status, 500);
}
