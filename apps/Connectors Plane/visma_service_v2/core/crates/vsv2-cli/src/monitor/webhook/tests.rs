//! The POST to the webhook: the body Slack and Teams expect, the timeout, and above all that no
//! failure message carries the URL.

use std::net::TcpListener;
use std::time::Duration;

use mockito::Matcher;
use serde_json::json;

use super::*;

/// A path that looks like a Slack token; it must never appear in a message.
const TOKEN: &str = "topsecretT0kenValue";

fn secret(text: &str) -> Secret {
    text.parse().expect("a Secret parses from any text")
}

/// A local address nothing listens on.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a free port");
    listener.local_addr().expect("local address").port()
}

fn assert_clean(message: &str, url: &str) {
    assert!(!message.contains(url), "the URL is in: {message}");
    assert!(!message.contains(TOKEN), "the token is in: {message}");
}

#[test]
fn the_delivery_timeout_is_ten_seconds() {
    assert_eq!(TIMEOUT, Duration::from_secs(10));
}

#[tokio::test]
async fn the_message_is_posted_as_json_with_a_text_field() {
    let mut server = mockito::Server::new_async().await;
    let hook = server
        .mock("POST", format!("/services/T0/B0/{TOKEN}").as_str())
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(json!({ "text": "line one\nline two" })))
        .with_status(200)
        .with_body("ok")
        .expect(1)
        .create_async()
        .await;
    let url = secret(&format!("{}/services/T0/B0/{TOKEN}", server.url()));

    let result = post_text(&url, "line one\nline two", TIMEOUT).await;

    assert_eq!(result, Ok(()));
    hook.assert_async().await;
}

#[tokio::test]
async fn any_2xx_answer_is_a_delivery() {
    for status in [200, 201, 202, 204] {
        let mut server = mockito::Server::new_async().await;
        let _hook = server
            .mock("POST", "/hook")
            .with_status(status)
            .create_async()
            .await;
        let url = secret(&format!("{}/hook", server.url()));

        let result = post_text(&url, "x", TIMEOUT).await;

        assert_eq!(result, Ok(()), "HTTP {status}");
    }
}

#[tokio::test]
async fn an_error_status_is_reported_by_its_number_and_not_by_its_url() {
    let mut server = mockito::Server::new_async().await;
    let _hook = server
        .mock("POST", format!("/services/T0/B0/{TOKEN}").as_str())
        .with_status(500)
        .with_body("the secret is in the path")
        .create_async()
        .await;
    let url = format!("{}/services/T0/B0/{TOKEN}", server.url());

    let message = post_text(&secret(&url), "x", TIMEOUT)
        .await
        .expect_err("500 is a failure");

    assert_eq!(message, "the webhook answered HTTP 500");
    assert_clean(&message, &url);
}

#[tokio::test]
async fn a_redirect_is_a_failure_and_is_not_followed() {
    let mut server = mockito::Server::new_async().await;
    let _hook = server
        .mock("POST", "/hook")
        .with_status(302)
        .with_header("location", "http://127.0.0.1:9/elsewhere")
        .create_async()
        .await;
    let url = secret(&format!("{}/hook", server.url()));

    let message = post_text(&url, "x", TIMEOUT)
        .await
        .expect_err("302 is a failure");

    assert_eq!(message, "the webhook answered HTTP 302");
}

#[tokio::test]
async fn a_refused_connection_is_reported_without_the_url() {
    let url = format!("http://127.0.0.1:{}/services/T0/B0/{TOKEN}", closed_port());

    let message = post_text(&secret(&url), "x", TIMEOUT)
        .await
        .expect_err("nothing listens there");

    assert!(
        message.starts_with("could not connect to the webhook"),
        "{message}"
    );
    assert_clean(&message, &url);
    assert!(
        !message.contains("127.0.0.1"),
        "not even the host: {message}"
    );
}

#[tokio::test]
async fn a_webhook_that_does_not_answer_times_out_and_the_message_says_so() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local address").port();
    // Accepts the connection and then says nothing.
    let _silent = std::thread::spawn(move || {
        let _held = listener.accept();
        std::thread::sleep(Duration::from_secs(2));
    });
    let url = format!("http://127.0.0.1:{port}/services/T0/B0/{TOKEN}");

    let message = post_text(&secret(&url), "x", Duration::from_millis(300))
        .await
        .expect_err("no answer comes");

    assert!(
        message.starts_with("the webhook did not answer in time"),
        "{message}"
    );
    assert_clean(&message, &url);
}

#[tokio::test]
async fn text_that_is_not_a_url_is_rejected_without_being_echoed() {
    let bad = format!("not a url {TOKEN}");

    let message = post_text(&secret(&bad), "x", TIMEOUT)
        .await
        .expect_err("not a URL");

    assert_eq!(message, "the webhook URL is not a valid URL");
    assert_clean(&message, &bad);
}

#[tokio::test]
async fn only_http_and_https_are_accepted_and_the_rest_is_not_echoed() {
    let url = format!("ftp://hooks.example.com/services/{TOKEN}");

    let message = post_text(&secret(&url), "x", TIMEOUT)
        .await
        .expect_err("ftp is not a webhook");

    assert!(message.contains("http:// or https://"), "{message}");
    assert_clean(&message, &url);
}

#[tokio::test]
async fn surrounding_whitespace_in_the_url_is_ignored() {
    let mut server = mockito::Server::new_async().await;
    let hook = server
        .mock("POST", "/hook")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let url = secret(&format!("  {}/hook \n", server.url()));

    let result = post_text(&url, "x", TIMEOUT).await;

    assert_eq!(result, Ok(()));
    hook.assert_async().await;
}
