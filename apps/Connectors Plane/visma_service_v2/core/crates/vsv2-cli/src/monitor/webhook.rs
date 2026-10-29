//! The one HTTP POST the monitor makes: the summary to a Slack or Teams incoming webhook.
//!
//! The URL is a credential (the token is in the path). Nothing in this file returns or logs it:
//! a failure comes back as a message that names what went wrong, with the URL, its host, its path
//! and its query masked, so the caller may print it.

use std::time::Duration;

use serde_json::json;

use super::secret::{redact, url_secrets, Secret};

/// The longest a delivery may take, connecting included.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// POST `{"text": text}` to `url` and return `Ok` on any 2xx.
///
/// The `Err` text is safe to print: it never contains the URL. Redirects are not followed (a
/// webhook that redirects is a misconfigured one, and the message would go to a third place).
pub async fn post_text(url: &Secret, text: &str, timeout: Duration) -> Result<(), String> {
    let secrets = url_secrets(url.expose());
    let target = reqwest::Url::parse(url.expose().trim())
        .map_err(|_| "the webhook URL is not a valid URL".to_string())?;
    if !matches!(target.scheme(), "http" | "https") {
        return Err("the webhook URL must start with http:// or https://".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("vsv2-monitor/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| describe(e, &secrets))?;
    let response = client
        .post(target)
        .json(&json!({ "text": text }))
        .send()
        .await
        .map_err(|e| describe(e, &secrets))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("the webhook answered HTTP {}", status.as_u16()))
    }
}

/// What went wrong with a request, in words, with the URL taken out: reqwest names the URL in its
/// message, so it is removed from the error first, and anything that still looks like it is masked.
fn describe(error: reqwest::Error, secrets: &[String]) -> String {
    let what = if error.is_timeout() {
        "the webhook did not answer in time"
    } else if error.is_connect() {
        "could not connect to the webhook"
    } else {
        "the webhook request failed"
    };
    let error = error.without_url();
    let mut causes = vec![error.to_string()];
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }
    redact(&format!("{what}: {}", causes.join(": ")), secrets)
}

#[cfg(test)]
mod tests;
