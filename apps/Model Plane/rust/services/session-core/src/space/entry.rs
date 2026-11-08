//! Typed entries of a human post (build plan WS15-S9).
//!
//! A post can carry one [`pb::MessageEntry`]: a closed `kind`, a `version` and the kind's payload.
//! Today's only kind is `link_previews` (text-only previews snapshotted when the post was sent).
//!
//! - [`validate_entry`] is the write rule. `append_message_in_tx_as` calls it for a human post
//!   (`post_kind = 'post'`) only and refuses an entry on every other append. It returns the JSON
//!   stored under `messages.metadata.entry`, so the entry shares the message's audience columns,
//!   erasure and retention (no table, no migration).
//! - [`entry_from_stored`] is the read rule. It rebuilds the typed message from what the column
//!   holds and returns nothing for a value that does not pass the same structural rules, so a
//!   free `entry` key in some other writer's metadata is never surfaced as a typed entry. An
//!   unknown future `kind` (or a newer `version`) comes back as `{kind, version}` with no payload,
//!   so an older reader shows its neutral fallback.
//!
//! Both are pure (no I/O). The caller's `metadata` can never set the key: the append path strips
//! `entry` from client metadata before it stores anything.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use mp_contracts::model_plane::v1 as pb;
use serde_json::{json, Value};

/// The metadata key the entry is stored under. Reserved: stripped from client metadata on write.
pub(crate) const ENTRY_METADATA_KEY: &str = "entry";
/// The only typed kind today.
pub(crate) const KIND_LINK_PREVIEWS: &str = "link_previews";
const ENTRY_VERSION: u32 = 1;
const MAX_LINK_PREVIEWS: usize = 3;
const MAX_URL_BYTES: usize = 2048;
const MAX_TITLE_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 400;
const MAX_SITE_NAME_CHARS: usize = 80;
/// `fetched_at` may be this far ahead of the server clock (clock skew between planes).
const FETCHED_AT_SKEW_MINUTES: i64 = 5;
/// Longest unknown `kind` a reader echoes back.
const MAX_UNKNOWN_KIND_CHARS: usize = 40;

/// The host of an http(s) URL, lowercased, or `None` when the URL is not an acceptable link: the
/// scheme is not http or https, it is empty or longer than 2048 bytes, it holds whitespace or a
/// control character, it carries userinfo (`user:pass@`) or it has no host.
fn http_url_host(url: &str) -> Option<String> {
    if url.is_empty() || url.len() > MAX_URL_BYTES {
        return None;
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url["https://".len()..]
    } else if lower.starts_with("http://") {
        &url["http://".len()..]
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return None;
    }
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        // [v6]:port
        let end = bracketed.find(']')?;
        &authority[..end + 2]
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.is_empty() || host == "[]" {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// The text with control characters removed and the ends trimmed, refused when it is longer than
/// `max_chars`.
fn clean_text(text: &str, max_chars: usize) -> Result<String, ()> {
    let cleaned: String = text.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.chars().count() > max_chars {
        return Err(());
    }
    Ok(cleaned)
}

fn timestamp_to_utc(ts: &prost_types::Timestamp) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(ts.seconds, u32::try_from(ts.nanos).ok()?)
}

/// One preview, validated and normalised into its stored JSON. `now` is `None` on a read, where
/// the clock rule does not apply.
fn check_link_preview(preview: &pb::LinkPreview, now: Option<DateTime<Utc>>) -> Result<Value, &'static str> {
    let url = preview.url.trim();
    let final_url = if preview.final_url.trim().is_empty() {
        url
    } else {
        preview.final_url.trim()
    };
    if http_url_host(url).is_none() {
        return Err("entry link preview url must be an http or https link without credentials");
    }
    // The host is recomputed from the final URL; a client-sent host is ignored.
    let Some(host) = http_url_host(final_url) else {
        return Err("entry link preview final_url must be an http or https link without credentials");
    };
    let title = clean_text(&preview.title, MAX_TITLE_CHARS).map_err(|()| "entry link preview title is too long")?;
    let description = clean_text(&preview.description, MAX_DESCRIPTION_CHARS)
        .map_err(|()| "entry link preview description is too long")?;
    let site_name =
        clean_text(&preview.site_name, MAX_SITE_NAME_CHARS).map_err(|()| "entry link preview site_name is too long")?;
    let fetched_at = preview
        .fetched_at
        .as_ref()
        .and_then(timestamp_to_utc)
        .ok_or("entry link preview fetched_at is required")?;
    if now.is_some_and(|now| fetched_at > now + Duration::minutes(FETCHED_AT_SKEW_MINUTES)) {
        return Err("entry link preview fetched_at is in the future");
    }
    Ok(json!({
        "url": url,
        "final_url": final_url,
        "host": host,
        "title": title,
        "description": description,
        "site_name": site_name,
        "fetched_at": fetched_at.to_rfc3339_opts(SecondsFormat::Micros, true),
    }))
}

fn check_entry(entry: &pb::MessageEntry, now: Option<DateTime<Utc>>) -> Result<Value, &'static str> {
    if entry.kind != KIND_LINK_PREVIEWS {
        return Err("entry kind is not supported");
    }
    if entry.version != ENTRY_VERSION {
        return Err("entry version is not supported");
    }
    if entry.link_previews.is_empty() || entry.link_previews.len() > MAX_LINK_PREVIEWS {
        return Err("entry carries one to three link previews");
    }
    let previews = entry
        .link_previews
        .iter()
        .map(|preview| check_link_preview(preview, now))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "kind": KIND_LINK_PREVIEWS,
        "version": ENTRY_VERSION,
        "link_previews": previews,
    }))
}

/// Validates the entry of a human post against the server clock `now` and returns the JSON to
/// store under `messages.metadata.entry`. The error text is a closed sentence for an
/// `InvalidArgument` status; it never repeats the caller's content.
pub(crate) fn validate_entry(entry: &pb::MessageEntry, now: DateTime<Utc>) -> Result<Value, &'static str> {
    check_entry(entry, Some(now))
}

fn string_of(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn link_preview_from_stored(value: &Value) -> Option<pb::LinkPreview> {
    let fetched_at = DateTime::parse_from_rfc3339(value.get("fetched_at")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    Some(pb::LinkPreview {
        url: string_of(value, "url"),
        final_url: string_of(value, "final_url"),
        host: string_of(value, "host"),
        title: string_of(value, "title"),
        description: string_of(value, "description"),
        site_name: string_of(value, "site_name"),
        fetched_at: Some(prost_types::Timestamp {
            seconds: fetched_at.timestamp(),
            nanos: i32::try_from(fetched_at.timestamp_subsec_nanos()).unwrap_or(0),
        }),
    })
}

fn is_unknown_kind_word(kind: &str) -> bool {
    !kind.is_empty()
        && kind.chars().count() <= MAX_UNKNOWN_KIND_CHARS
        && kind.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Rebuilds the typed entry from the stored `metadata.entry` value. `None` when there is no entry
/// or the value is not one this writer could have stored.
pub(crate) fn entry_from_stored(stored: &Value) -> Option<pb::MessageEntry> {
    let kind = stored.get("kind")?.as_str()?;
    let version = u32::try_from(stored.get("version")?.as_u64()?).ok()?;
    if kind != KIND_LINK_PREVIEWS || version != ENTRY_VERSION {
        // A kind or version this build does not know: `{kind, version}` and no payload.
        return is_unknown_kind_word(kind).then(|| pb::MessageEntry {
            kind: kind.to_owned(),
            version,
            link_previews: Vec::new(),
        });
    }
    let link_previews = stored
        .get("link_previews")?
        .as_array()?
        .iter()
        .map(link_preview_from_stored)
        .collect::<Option<Vec<_>>>()?;
    let entry = pb::MessageEntry {
        kind: kind.to_owned(),
        version,
        link_previews,
    };
    // The same rules as the write, without the clock rule.
    check_entry(&entry, None).ok()?;
    Some(entry)
}
