//! Text a person wrote (a prompt, a brief, the material under review), held so that it cannot
//! reach a log, a metric label or a debug print by accident (F-t3-075).
//!
//! A [`UserContent`] has no `Serialize` impl. Under `Debug` and `Display` it prints only its
//! size, as `[user content: <n> bytes]`. It has exactly two ways out,
//! [`UserContent::expose_for_model`] and [`UserContent::expose_for_storage`], and the name of each
//! says why the text is leaving. So a tracing field, a `format!` or a derived `Debug` on a struct
//! that holds one shows the size, never the words. It deserializes transparently from a JSON
//! string, so a request body can hold it directly.

use serde::Deserialize;
use std::fmt;

/// Text a person wrote. See the module docs for what the type refuses to do.
///
/// The only ways out are [`UserContent::expose_for_model`] and
/// [`UserContent::expose_for_storage`]. A third accessor needs a privacy review first.
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct UserContent(String);

/// The retention posture a stored copy of a [`UserContent`] is written under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoragePosture {
    /// The turn is persistent: its org is not zero retention, so the text may be kept.
    Persistent,
    /// Zero data retention, or a waiver for one connection. Nothing may be kept.
    ZeroRetention,
}

impl UserContent {
    /// The text for the model, and for the gateway's own in-process work on the request and its
    /// reply: the checks, the content hash and the card that goes back to the same caller.
    ///
    /// Use it to build the model request or to work on that request in process. Never pass the
    /// result to a logger, a metric, an error message or a store. A store takes
    /// [`UserContent::expose_for_storage`] instead. This is one of the only two ways out of a
    /// [`UserContent`].
    #[must_use]
    pub fn expose_for_model(&self) -> &str {
        self.0.as_str()
    }

    /// The text for a store, under the posture the turn runs under.
    ///
    /// Under [`StoragePosture::Persistent`] this is the text. Under
    /// [`StoragePosture::ZeroRetention`] it is the empty string, so a store that writes the result
    /// keeps no content. A store should skip the write when the result is empty. This is the other
    /// of the only two ways out of a [`UserContent`].
    #[must_use]
    pub fn expose_for_storage(&self, posture: StoragePosture) -> &str {
        match posture {
            StoragePosture::Persistent => self.0.as_str(),
            StoragePosture::ZeroRetention => "",
        }
    }

    /// The length of the text in bytes. A number, so it is safe to log.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for UserContent {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl fmt::Debug for UserContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[user content: {} bytes]", self.0.len())
    }
}

impl fmt::Display for UserContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[user content: {} bytes]", self.0.len())
    }
}
