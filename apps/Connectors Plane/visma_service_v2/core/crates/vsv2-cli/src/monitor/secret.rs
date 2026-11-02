//! Text that must never reach the output: the webhook URL, the database URL.

use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

/// What replaces a secret in a message.
const MASK: &str = "***";

/// Secrets shorter than this are not masked: they would match ordinary words.
const MIN_MASKED_LEN: usize = 4;

/// A value whose text must stay out of logs and reports: `Debug` and `Display` print asterisks, so
/// even a `{:?}` of the arguments cannot leak it. Read it with [`Secret::expose`].
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// An empty value (an environment variable that is set but blank) counts as no value.
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl FromStr for Secret {
    type Err = Infallible;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self(text.to_string()))
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({MASK})")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MASK)
    }
}

/// Every piece of a URL that identifies it and so must not be printed: the whole text, the
/// password, the path and query (where webhook tokens live) and the host.
pub fn url_secrets(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    let mut secrets = vec![raw.to_string()];
    if let Ok(url) = reqwest::Url::parse(raw) {
        secrets.extend(url.password().map(str::to_string));
        secrets.extend(url.host_str().map(str::to_string));
        secrets.push(url.path().to_string());
        secrets.extend(url.query().map(str::to_string));
    }
    secrets
}

/// `text` with every secret replaced by asterisks, the longest first so that a URL is masked as a
/// whole before its parts are.
pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut ordered: Vec<&String> = secrets
        .iter()
        .filter(|secret| secret.len() >= MIN_MASKED_LEN)
        .collect();
    ordered.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    ordered
        .into_iter()
        .fold(text.to_string(), |text, secret| text.replace(secret, MASK))
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://hooks.example.com/services/T000/B000/s3cr3tT0ken?sig=abc123";

    #[test]
    fn a_secret_prints_as_asterisks_in_every_formatter() {
        let secret = Secret(URL.to_string());

        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert_eq!(format!("{secret}"), "***");
        assert_eq!(secret.expose(), URL);
    }

    #[test]
    fn a_blank_value_counts_as_none() {
        assert!(Secret(String::new()).is_blank());
        assert!(Secret("  \t".into()).is_blank());
        assert!(!Secret("x".into()).is_blank());
    }

    #[test]
    fn redact_masks_the_whole_url() {
        let message = format!("error sending request for url ({URL})");

        let clean = redact(&message, &url_secrets(URL));

        assert_eq!(clean, "error sending request for url (***)");
    }

    #[test]
    fn redact_masks_the_parts_of_a_url_that_show_up_on_their_own() {
        let message = "dns error for hooks.example.com; request path \
                       /services/T000/B000/s3cr3tT0ken?sig=abc123 failed";

        let clean = redact(message, &url_secrets(URL));

        assert!(!clean.contains("s3cr3tT0ken"), "{clean}");
        assert!(!clean.contains("hooks.example.com"), "{clean}");
        assert!(!clean.contains("abc123"), "{clean}");
        assert!(clean.contains("***"), "{clean}");
    }

    #[test]
    fn the_password_of_a_database_url_is_masked() {
        let dsn = "postgres://visma_v2:hunter2pw@db.internal:5432/visma_v2";
        let message = format!("cannot connect to {dsn} (password hunter2pw)");

        let clean = redact(&message, &url_secrets(dsn));

        assert!(!clean.contains("hunter2pw"), "{clean}");
        assert!(!clean.contains(dsn), "{clean}");
    }

    #[test]
    fn short_secrets_are_left_alone_so_ordinary_words_survive() {
        let clean = redact("a b c: /x is fine", &["/x".to_string(), "b".to_string()]);

        assert_eq!(clean, "a b c: /x is fine");
    }

    #[test]
    fn text_that_is_not_a_url_is_masked_as_it_stands() {
        let clean = redact(
            "bad value not-a-url-at-all here",
            &url_secrets("not-a-url-at-all"),
        );

        assert_eq!(clean, "bad value *** here");
    }
}
