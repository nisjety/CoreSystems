//! Typed entries of a human post (build plan WS15-S9).
//!
//! A post can carry one [`pb::MessageEntry`]: a closed `kind`, a `version` and the kind's payload.
//! Today's kinds are `link_previews` (text-only previews snapshotted when the post was sent) and
//! `agent_card` (WS18-S7: the plain-text title, tone and fields a room agent wrote with
//! `space.thread.post_card`; its body is the row's content). Only the card path writes `agent_card`:
//! the human write rule refuses it, so a person's post can never carry a card.
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
/// The link preview kind (WS15-S9).
pub(crate) const KIND_LINK_PREVIEWS: &str = "link_previews";
/// The agent card kind (WS18-S7). Written only by `space::self_api`, never by a human post.
pub(crate) const KIND_AGENT_CARD: &str = "agent_card";
const ENTRY_VERSION: u32 = 1;
const AGENT_CARD_VERSION: u32 = 1;
/// The card's tones, as the golden `space.thread.post_card` schema names them.
pub(crate) const CARD_TONES: [&str; 4] = ["info", "progress", "done", "blocked"];
pub(crate) const CARD_TITLE_MAX_CHARS: usize = 120;
pub(crate) const CARD_BODY_MAX_CHARS: usize = 2000;
pub(crate) const CARD_FIELDS_MAX: usize = 8;
pub(crate) const CARD_FIELD_LABEL_MAX_CHARS: usize = 40;
pub(crate) const CARD_FIELD_VALUE_MAX_CHARS: usize = 200;
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

/// Whether a card's text is plain (WS18-S7): non-blank, within `min` to `max` characters, with no
/// control character (a line break only when `multiline`) and no link (`://` or `www.`, in any case).
/// The SPA draws card text as text, so a card never carries a link or a markup it would have to render.
pub(crate) fn plain_card_text(text: &str, min: usize, max: usize, multiline: bool) -> bool {
    let count = text.chars().count();
    if count < min || count > max || text.trim().is_empty() {
        return false;
    }
    if text.chars().any(|c| c.is_control() && !(multiline && c == '\n')) {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    !(lower.contains("://") || lower.contains("www."))
}

/// The JSON stored under `messages.metadata.entry` for an agent card (WS18-S7). The caller has already
/// checked the arguments against the golden schema; the body is not repeated here (it is the content).
pub(crate) fn agent_card_json(title: &str, tone: &str, fields: &[(String, String)]) -> Value {
    json!({
        "kind": KIND_AGENT_CARD,
        "version": AGENT_CARD_VERSION,
        "title": title,
        "tone": tone,
        "fields": fields
            .iter()
            .map(|(label, value)| json!({ "label": label, "value": value }))
            .collect::<Vec<_>>(),
    })
}

/// The card payload read back from the stored JSON. `None` when it does not pass the write rules; the
/// reader then keeps the kind and version only, so the body still draws as plain text.
fn agent_card_from_stored(stored: &Value) -> Option<pb::AgentCard> {
    let title = stored.get("title")?.as_str()?;
    let tone = stored.get("tone")?.as_str()?;
    if !plain_card_text(title, 1, CARD_TITLE_MAX_CHARS, false) || !CARD_TONES.contains(&tone) {
        return None;
    }
    let fields = stored.get("fields")?.as_array()?;
    if fields.len() > CARD_FIELDS_MAX {
        return None;
    }
    let fields = fields
        .iter()
        .map(|field| {
            let label = field.get("label")?.as_str()?;
            let value = field.get("value")?.as_str()?;
            (plain_card_text(label, 1, CARD_FIELD_LABEL_MAX_CHARS, false)
                && plain_card_text(value, 1, CARD_FIELD_VALUE_MAX_CHARS, false))
            .then(|| pb::AgentCardField {
                label: label.to_owned(),
                value: value.to_owned(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(pb::AgentCard {
        title: title.to_owned(),
        tone: tone.to_owned(),
        fields,
    })
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
    if kind == KIND_AGENT_CARD {
        // A card keeps its kind and version even when its payload fails the rules, so the reader draws
        // the body as plain text. Only version 1 carries a payload; a newer version is kind and version only.
        let agent_card = if version == AGENT_CARD_VERSION {
            agent_card_from_stored(stored)
        } else {
            None
        };
        return Some(pb::MessageEntry {
            kind: kind.to_owned(),
            version,
            link_previews: Vec::new(),
            agent_card,
        });
    }
    if kind != KIND_LINK_PREVIEWS || version != ENTRY_VERSION {
        // A kind or version this build does not know: `{kind, version}` and no payload.
        return is_unknown_kind_word(kind).then(|| pb::MessageEntry {
            kind: kind.to_owned(),
            version,
            link_previews: Vec::new(),
            agent_card: None,
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
        agent_card: None,
    };
    // The same rules as the write, without the clock rule.
    check_entry(&entry, None).ok()?;
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    const URL_REFUSED: &str = "entry link preview url must be an http or https link without credentials";
    const FINAL_URL_REFUSED: &str = "entry link preview final_url must be an http or https link without credentials";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).single().expect("test time")
    }

    fn stamp(at: DateTime<Utc>) -> prost_types::Timestamp {
        prost_types::Timestamp {
            seconds: at.timestamp(),
            nanos: i32::try_from(at.timestamp_subsec_nanos()).unwrap_or(0),
        }
    }

    /// A valid preview of `url`, fetched a minute before [`now`].
    fn preview(url: &str) -> pb::LinkPreview {
        pb::LinkPreview {
            url: url.to_owned(),
            final_url: String::new(),
            host: String::new(),
            title: "A title".to_owned(),
            description: "A description".to_owned(),
            site_name: "Example".to_owned(),
            fetched_at: Some(stamp(now() - Duration::minutes(1))),
        }
    }

    fn entry_of(previews: Vec<pb::LinkPreview>) -> pb::MessageEntry {
        pb::MessageEntry {
            kind: KIND_LINK_PREVIEWS.to_owned(),
            version: 1,
            link_previews: previews,
            agent_card: None,
        }
    }

    fn accepted(preview: pb::LinkPreview) -> Value {
        validate_entry(&entry_of(vec![preview]), now()).expect("the entry must be accepted")
    }

    fn refusal_of(entry: &pb::MessageEntry) -> &'static str {
        validate_entry(entry, now()).expect_err("the entry must be refused")
    }

    fn refusal(preview: pb::LinkPreview) -> &'static str {
        refusal_of(&entry_of(vec![preview]))
    }

    // ---- validate_entry: the write rule -----------------------------------------------------

    #[test]
    fn a_valid_entry_is_stored_with_its_host_recomputed_and_its_time_in_microseconds() {
        let mut sent = preview("https://Example.COM/a/b?q=1#frag");
        sent.host = "client-sent.example".to_owned();
        assert_eq!(
            accepted(sent),
            json!({
                "kind": "link_previews",
                "version": 1,
                "link_previews": [{
                    "url": "https://Example.COM/a/b?q=1#frag",
                    "final_url": "https://Example.COM/a/b?q=1#frag",
                    "host": "example.com",
                    "title": "A title",
                    "description": "A description",
                    "site_name": "Example",
                    "fetched_at": "2026-11-01T11:59:00.000000Z",
                }],
            })
        );
    }

    #[test]
    fn one_to_three_previews_are_accepted_and_zero_or_four_or_more_are_refused() {
        for count in [1_usize, 2, 3] {
            let previews = (0..count).map(|n| preview(&format!("https://example.com/{n}"))).collect();
            let stored = validate_entry(&entry_of(previews), now()).expect("accepted");
            assert_eq!(stored["link_previews"].as_array().map(Vec::len), Some(count), "{count} previews");
        }
        for count in [0_usize, 4, 10] {
            let previews = (0..count).map(|n| preview(&format!("https://example.com/{n}"))).collect();
            assert_eq!(
                refusal_of(&entry_of(previews)),
                "entry carries one to three link previews",
                "{count} previews"
            );
        }
        // Every preview is held to the rules, not only the first.
        let previews = vec![
            preview("https://example.com/1"),
            preview("https://example.com/2"),
            preview("ftp://example.com/3"),
        ];
        assert_eq!(refusal_of(&entry_of(previews)), URL_REFUSED);
    }

    #[test]
    fn only_the_link_previews_kind_at_version_one_is_a_human_entry() {
        for kind in ["", "agent_card", "link_preview", "LINK_PREVIEWS", " link_previews", "poll"] {
            let mut entry = entry_of(vec![preview("https://example.com/")]);
            entry.kind = kind.to_owned();
            assert_eq!(refusal_of(&entry), "entry kind is not supported", "kind {kind:?}");
        }
        for version in [0, 2, 3, u32::MAX] {
            let mut entry = entry_of(vec![preview("https://example.com/")]);
            entry.version = version;
            assert_eq!(refusal_of(&entry), "entry version is not supported", "version {version}");
        }
    }

    #[test]
    fn a_human_post_can_never_carry_an_agent_card() {
        let card = pb::AgentCard {
            title: "Done".to_owned(),
            tone: "done".to_owned(),
            fields: Vec::new(),
        };
        let entry = pb::MessageEntry {
            kind: KIND_AGENT_CARD.to_owned(),
            version: AGENT_CARD_VERSION,
            link_previews: Vec::new(),
            agent_card: Some(card),
        };
        assert_eq!(refusal_of(&entry), "entry kind is not supported");
        // Link previews riding along under the card's kind do not make it a human entry either.
        let mut with_previews = entry.clone();
        with_previews.link_previews = vec![preview("https://example.com/")];
        assert_eq!(refusal_of(&with_previews), "entry kind is not supported");
    }

    #[test]
    fn only_http_and_https_links_are_accepted_whatever_the_case() {
        for url in [
            "ftp://example.com/x",
            "javascript:alert(1)",
            "data:text/html,hi",
            "file:///etc/passwd",
            "mailto:a@b.example",
            "//example.com/x",
            "example.com/x",
            "httpx://example.com",
            "https:/example.com",
            "https//example.com",
            "",
            "   ",
        ] {
            assert_eq!(refusal(preview(url)), URL_REFUSED, "{url:?}");
        }
        for (url, host) in [
            ("http://example.com", "example.com"),
            ("https://example.com", "example.com"),
            ("HTTP://EXAMPLE.COM/x", "example.com"),
            ("Https://Example.com:8443/x?y=1#z", "example.com"),
            ("https://sub.example.co.uk/path", "sub.example.co.uk"),
            ("https://[::1]:8080/x", "[::1]"),
            ("https://example.com?x=a@b", "example.com"),
            ("https://example.com/path@x", "example.com"),
            ("https://example.com#a@b", "example.com"),
        ] {
            let stored = accepted(preview(url));
            assert_eq!(stored["link_previews"][0]["host"], host, "{url:?}");
        }
    }

    #[test]
    fn a_link_with_userinfo_is_refused_and_the_refusal_never_repeats_it() {
        for url in [
            "https://user:pass@example.com/",
            "https://user@example.com",
            "http://@example.com",
            "https://a:b@[::1]/",
            "https://example.com\\@evil.example/",
            "https://evil.example\\.example.com/",
        ] {
            assert_eq!(refusal(preview(url)), URL_REFUSED, "{url:?}");
        }
        let message = refusal(preview("https://user:CANARY-SECRET@example.com/"));
        assert!(!message.contains("CANARY"), "the refusal is a closed sentence: {message}");
    }

    #[test]
    fn a_link_without_a_host_is_refused() {
        for url in [
            "https://",
            "http://",
            "https:///path",
            "http://:80/",
            "https://?q=1",
            "https://#frag",
            "https://[]/",
            "https://[/",
        ] {
            assert_eq!(refusal(preview(url)), URL_REFUSED, "{url:?}");
        }
    }

    #[test]
    fn a_link_is_at_most_2048_bytes() {
        let stem = "https://example.com/";
        let at_cap = format!("{stem}{}", "a".repeat(MAX_URL_BYTES - stem.len()));
        assert_eq!(at_cap.len(), MAX_URL_BYTES);
        accepted(preview(&at_cap));
        let over = format!("{at_cap}a");
        assert_eq!(refusal(preview(&over)), URL_REFUSED);
        // The final URL is held to the same cap, with its own sentence.
        let mut long_final = preview("https://example.com/");
        long_final.final_url = over;
        assert_eq!(refusal(long_final), FINAL_URL_REFUSED);
    }

    #[test]
    fn a_link_with_whitespace_or_a_control_character_inside_is_refused_but_the_ends_are_trimmed() {
        for url in [
            "https://exa mple.com/",
            "https://example.com/a b",
            "https://example.com/\tx",
            "https://example.com/\nx",
            "https://example.com/\u{0}",
            "https://example.com/\u{7f}",
            "https://example.com/\u{a0}x",
            "https://example.com/\u{2003}x",
        ] {
            assert_eq!(refusal(preview(url)), URL_REFUSED, "{url:?}");
        }
        let stored = accepted(preview(" \thttps://example.com/ok\t "));
        assert_eq!(stored["link_previews"][0]["url"], "https://example.com/ok");
    }

    #[test]
    fn the_host_is_recomputed_from_the_final_url_and_a_client_host_is_ignored() {
        let mut moved = preview("https://short.example/a");
        moved.final_url = "  https://Final.Example.ORG:8443/landing  ".to_owned();
        moved.host = "evil.example".to_owned();
        let stored = accepted(moved);
        let first = &stored["link_previews"][0];
        assert_eq!(first["host"], "final.example.org");
        assert_eq!(first["url"], "https://short.example/a");
        assert_eq!(first["final_url"], "https://Final.Example.ORG:8443/landing");

        // A blank final URL means the URL itself.
        let mut blank = preview("https://Short.Example/a");
        blank.final_url = "   ".to_owned();
        blank.host = "evil.example".to_owned();
        let stored = accepted(blank);
        let first = &stored["link_previews"][0];
        assert_eq!(first["host"], "short.example");
        assert_eq!(first["final_url"], "https://Short.Example/a");

        // A final URL that is not an acceptable link is refused with its own sentence.
        for final_url in ["ftp://final.example/", "https://user:pw@final.example/", "javascript:alert(1)", "final.example"] {
            let mut bad = preview("https://short.example/a");
            bad.final_url = final_url.to_owned();
            assert_eq!(refusal(bad), FINAL_URL_REFUSED, "{final_url:?}");
        }
    }

    #[test]
    fn title_description_and_site_name_are_capped_at_200_400_and_80_characters() {
        let cases: [(&str, usize, &str); 3] = [
            ("title", MAX_TITLE_CHARS, "entry link preview title is too long"),
            ("description", MAX_DESCRIPTION_CHARS, "entry link preview description is too long"),
            ("site_name", MAX_SITE_NAME_CHARS, "entry link preview site_name is too long"),
        ];
        assert_eq!((MAX_TITLE_CHARS, MAX_DESCRIPTION_CHARS, MAX_SITE_NAME_CHARS), (200, 400, 80));
        for (field, max, message) in cases {
            let set = |preview: &mut pb::LinkPreview, text: String| match field {
                "title" => preview.title = text,
                "description" => preview.description = text,
                _ => preview.site_name = text,
            };
            // Characters are counted, not bytes: the cap holds for two-byte letters too.
            let mut at_cap = preview("https://example.com/");
            set(&mut at_cap, "\u{e9}".repeat(max));
            accepted(at_cap);
            let mut over = preview("https://example.com/");
            set(&mut over, "\u{e9}".repeat(max + 1));
            assert_eq!(refusal(over), message, "{field} over {max}");
        }
    }

    #[test]
    fn control_characters_are_removed_before_the_length_rule_and_the_ends_are_trimmed() {
        let mut messy = preview("https://example.com/");
        messy.title = "  He\u{0}llo\nWor\tld\u{7}  ".to_owned();
        messy.description = "\u{1b}[31mred\u{1b}[0m".to_owned();
        messy.site_name = "\r\nSite\r\n".to_owned();
        let stored = accepted(messy);
        let first = &stored["link_previews"][0];
        assert_eq!(first["title"], "HelloWorld", "control characters are removed, not replaced");
        assert_eq!(first["description"], "[31mred[0m");
        assert_eq!(first["site_name"], "Site");

        // Removed characters do not count toward the cap.
        let mut padded = preview("https://example.com/");
        padded.title = format!("{}{}", "a".repeat(MAX_TITLE_CHARS), "\u{0}".repeat(50));
        let stored = accepted(padded);
        assert_eq!(stored["link_previews"][0]["title"].as_str().map(str::len), Some(MAX_TITLE_CHARS));

        // Inner spaces are kept as written.
        let mut spaced = preview("https://example.com/");
        spaced.title = "Two  words".to_owned();
        assert_eq!(accepted(spaced)["link_previews"][0]["title"], "Two  words");
    }

    #[test]
    fn fetched_at_is_required_and_may_be_at_most_five_minutes_ahead() {
        let mut missing = preview("https://example.com/");
        missing.fetched_at = None;
        assert_eq!(refusal(missing), "entry link preview fetched_at is required");

        // A negative nanosecond field is not a time.
        let mut bad_nanos = preview("https://example.com/");
        bad_nanos.fetched_at = Some(prost_types::Timestamp { seconds: now().timestamp(), nanos: -1 });
        assert_eq!(refusal(bad_nanos), "entry link preview fetched_at is required");

        // Five minutes of clock skew between planes is allowed, one second more is not.
        let mut edge = preview("https://example.com/");
        edge.fetched_at = Some(stamp(now() + Duration::minutes(5)));
        accepted(edge);
        let mut future = preview("https://example.com/");
        future.fetched_at = Some(stamp(now() + Duration::minutes(5) + Duration::seconds(1)));
        assert_eq!(refusal(future), "entry link preview fetched_at is in the future");

        // The past has no limit.
        let mut old = preview("https://example.com/");
        old.fetched_at = Some(stamp(now() - Duration::days(3650)));
        accepted(old);
    }

    // ---- entry_from_stored: the read rule ---------------------------------------------------

    fn stored_entry() -> Value {
        accepted(preview("https://Example.com/a"))
    }

    /// The stored entry with `field` of its first preview replaced by `value`.
    fn with_first(field: &str, value: Value) -> Value {
        let mut stored = stored_entry();
        stored["link_previews"][0][field] = value;
        stored
    }

    #[test]
    fn a_stored_entry_reads_back_as_the_typed_entry() {
        let mut sent = preview("https://short.example/a");
        sent.final_url = "https://final.example/b".to_owned();
        sent.host = "ignored.example".to_owned();
        let read = entry_from_stored(&accepted(sent)).expect("a stored entry reads back");
        assert_eq!((read.kind.as_str(), read.version), ("link_previews", 1));
        assert!(read.agent_card.is_none());
        assert_eq!(read.link_previews.len(), 1);
        let first = &read.link_previews[0];
        assert_eq!(first.url, "https://short.example/a");
        assert_eq!(first.final_url, "https://final.example/b");
        assert_eq!(first.host, "final.example");
        assert_eq!(first.title, "A title");
        assert_eq!(first.description, "A description");
        assert_eq!(first.site_name, "Example");
        assert_eq!(first.fetched_at, Some(stamp(now() - Duration::minutes(1))));
    }

    #[test]
    fn an_unknown_kind_or_a_newer_version_reads_as_kind_and_version_only() {
        assert_eq!(
            entry_from_stored(&json!({"kind": "poll", "version": 3, "options": ["secret one", "secret two"]})),
            Some(pb::MessageEntry {
                kind: "poll".to_owned(),
                version: 3,
                link_previews: Vec::new(),
                agent_card: None,
            }),
            "the payload of an unknown kind never leaves the column"
        );
        // A newer version of a known kind is neutral too: this build cannot vouch for its payload.
        let newer = json!({
            "kind": "link_previews",
            "version": 2,
            "link_previews": [{"url": "https://example.com/", "title": "secret"}],
        });
        assert_eq!(
            entry_from_stored(&newer),
            Some(pb::MessageEntry {
                kind: "link_previews".to_owned(),
                version: 2,
                link_previews: Vec::new(),
                agent_card: None,
            })
        );
        let zero = entry_from_stored(&json!({"kind": "link_previews", "version": 0})).expect("neutral");
        assert_eq!((zero.kind.as_str(), zero.version, zero.link_previews.len()), ("link_previews", 0, 0));
    }

    #[test]
    fn an_unknown_kind_is_echoed_only_when_it_is_a_short_lowercase_word() {
        let longest = "a".repeat(MAX_UNKNOWN_KIND_CHARS);
        for kind in ["a", "poll", "poll_2", "x9", longest.as_str()] {
            let read = entry_from_stored(&json!({"kind": kind, "version": 1})).expect(kind);
            assert_eq!(read.kind, kind);
        }
        let too_long = "a".repeat(MAX_UNKNOWN_KIND_CHARS + 1);
        for kind in ["", "Poll", "po ll", "poll!", "poll-x", "p\u{f6}ll", too_long.as_str()] {
            assert_eq!(entry_from_stored(&json!({"kind": kind, "version": 1})), None, "{kind:?}");
        }
    }

    #[test]
    fn a_value_this_writer_could_not_have_stored_reads_as_nothing() {
        let four: Vec<Value> = (0..4)
            .map(|n| {
                let mut one = stored_entry()["link_previews"][0].clone();
                one["url"] = json!(format!("https://example.com/{n}"));
                one
            })
            .collect();
        let long_title = "t".repeat(MAX_TITLE_CHARS + 1);
        let cases: Vec<(&str, Value)> = vec![
            ("a string", json!("link_previews")),
            ("null", Value::Null),
            ("an array", json!([])),
            ("no kind", json!({"version": 1})),
            ("a kind that is not text", json!({"kind": 5, "version": 1})),
            ("no version", json!({"kind": "poll"})),
            ("a version that is not a number", json!({"kind": "poll", "version": "1"})),
            ("a negative version", json!({"kind": "poll", "version": -1})),
            ("a version beyond u32", json!({"kind": "poll", "version": 4_294_967_296_u64})),
            ("previews missing", json!({"kind": "link_previews", "version": 1})),
            ("previews that are not an array", json!({"kind": "link_previews", "version": 1, "link_previews": "x"})),
            ("an empty list of previews", json!({"kind": "link_previews", "version": 1, "link_previews": []})),
            ("four previews", json!({"kind": "link_previews", "version": 1, "link_previews": four})),
            ("a preview that is not an object", json!({"kind": "link_previews", "version": 1, "link_previews": ["x"]})),
            ("a preview without fetched_at", with_first("fetched_at", Value::Null)),
            ("a preview with an unparseable fetched_at", with_first("fetched_at", json!("yesterday"))),
            ("a preview with a non-http url", with_first("url", json!("javascript:alert(1)"))),
            ("a preview with credentials in its url", with_first("url", json!("https://u:p@example.com/"))),
            ("a preview whose final_url has credentials", with_first("final_url", json!("https://u:p@example.com/"))),
            ("a preview with an over-long title", with_first("title", json!(long_title))),
        ];
        for (why, stored) in cases {
            assert_eq!(entry_from_stored(&stored), None, "{why}");
        }
    }

    #[test]
    fn the_read_side_has_no_clock_rule() {
        // A preview fetched "in the future" by this process's clock still reads: the clock rule is a write rule.
        let read = entry_from_stored(&with_first("fetched_at", json!("2999-01-01T00:00:00Z"))).expect("reads");
        assert_eq!(read.link_previews.len(), 1);
    }

    // ---- WS18-S7: the agent card ------------------------------------------------------------

    #[test]
    fn plain_card_text_refuses_blank_over_long_control_and_link_text() {
        // (case, text, min, max, multiline, plain)
        let table: Vec<(&str, String, usize, usize, bool, bool)> = vec![
            ("ordinary text", "Build finished".to_owned(), 1, 120, false, true),
            ("exactly the minimum", "ab".to_owned(), 2, 120, false, true),
            ("under the minimum", "a".to_owned(), 2, 120, false, false),
            ("exactly the maximum", "a".repeat(120), 1, 120, false, true),
            ("over the maximum", "a".repeat(121), 1, 120, false, false),
            ("the maximum counts characters, not bytes", "\u{e9}".repeat(120), 1, 120, false, true),
            ("one character over in multi-byte text", "\u{e9}".repeat(121), 1, 120, false, false),
            ("empty", String::new(), 1, 120, false, false),
            ("only spaces", "    ".to_owned(), 1, 120, false, false),
            ("only line breaks in a body", "\n\n".to_owned(), 1, 2000, true, false),
            ("a tab", "a\tb".to_owned(), 1, 120, false, false),
            ("a bell", "a\u{7}b".to_owned(), 1, 120, false, false),
            ("a NUL", "a\u{0}b".to_owned(), 1, 120, false, false),
            ("an escape", "\u{1b}[31mred".to_owned(), 1, 120, false, false),
            ("a next-line control", "a\u{85}b".to_owned(), 1, 120, false, false),
            ("a line break in a title", "a\nb".to_owned(), 1, 120, false, false),
            ("a line break in a body", "line one\nline two".to_owned(), 1, 2000, true, true),
            ("a carriage return in a body", "line one\r\nline two".to_owned(), 1, 2000, true, false),
            ("a NUL in a body", "a\u{0}b".to_owned(), 1, 2000, true, false),
            ("a tab in a body", "a\tb".to_owned(), 1, 2000, true, false),
            ("a scheme link", "see https://example.com".to_owned(), 1, 120, false, false),
            ("a scheme link in capitals", "see HTTP://EXAMPLE.COM".to_owned(), 1, 120, false, false),
            ("any scheme link", "ftp://host".to_owned(), 1, 120, false, false),
            ("a www link", "see www.example.com".to_owned(), 1, 120, false, false),
            ("a www link in capitals", "WWW.Example.com".to_owned(), 1, 120, false, false),
            ("a www link inside a word", "awww.x".to_owned(), 1, 120, false, false),
            ("a link on the second line of a body", "first\nsee https://example.com".to_owned(), 1, 2000, true, false),
            ("www without a dot", "www is fine".to_owned(), 1, 120, false, true),
            ("a colon and slashes that are not a scheme", "ratio 3/4: done // ok".to_owned(), 1, 120, false, true),
        ];
        for (case, text, min, max, multiline, plain) in table {
            assert_eq!(plain_card_text(&text, min, max, multiline), plain, "{case}: {text:?}");
        }
    }

    fn card_fields(fields: &[(&str, &str)]) -> Vec<(String, String)> {
        fields.iter().map(|(label, value)| ((*label).to_owned(), (*value).to_owned())).collect()
    }

    #[test]
    fn an_agent_card_round_trips_through_its_stored_json() {
        let fields = card_fields(&[("Branch", "main"), ("Tests", "42 passed")]);
        let stored = agent_card_json("Build finished", "done", &fields);
        assert_eq!(
            stored,
            json!({
                "kind": "agent_card",
                "version": 1,
                "title": "Build finished",
                "tone": "done",
                "fields": [
                    {"label": "Branch", "value": "main"},
                    {"label": "Tests", "value": "42 passed"},
                ],
            })
        );
        assert_eq!(
            entry_from_stored(&stored),
            Some(pb::MessageEntry {
                kind: "agent_card".to_owned(),
                version: 1,
                link_previews: Vec::new(),
                agent_card: Some(pb::AgentCard {
                    title: "Build finished".to_owned(),
                    tone: "done".to_owned(),
                    fields: vec![
                        pb::AgentCardField { label: "Branch".to_owned(), value: "main".to_owned() },
                        pb::AgentCardField { label: "Tests".to_owned(), value: "42 passed".to_owned() },
                    ],
                }),
            })
        );
    }

    #[test]
    fn every_tone_and_the_largest_card_survive_the_round_trip() {
        assert_eq!(CARD_TONES, ["info", "progress", "done", "blocked"]);
        for tone in CARD_TONES {
            let read = entry_from_stored(&agent_card_json("T", tone, &[])).expect("reads");
            let card = read.agent_card.expect("a card with no fields");
            assert_eq!((card.title.as_str(), card.tone.as_str(), card.fields.len()), ("T", tone, 0));
        }
        // The largest card the write rules admit: 8 fields, a 120 character title, 40 and 200 character parts.
        let fields: Vec<(String, String)> = (0..CARD_FIELDS_MAX)
            .map(|_| ("l".repeat(CARD_FIELD_LABEL_MAX_CHARS), "v".repeat(CARD_FIELD_VALUE_MAX_CHARS)))
            .collect();
        let title = "t".repeat(CARD_TITLE_MAX_CHARS);
        let read = entry_from_stored(&agent_card_json(&title, "blocked", &fields)).expect("reads");
        let card = read.agent_card.expect("the largest card still reads");
        assert_eq!((card.title.chars().count(), card.fields.len()), (CARD_TITLE_MAX_CHARS, CARD_FIELDS_MAX));
    }

    #[test]
    fn a_bad_card_payload_or_a_newer_version_reads_as_kind_and_version_only() {
        let nine: Vec<Value> = (1..=9).map(|n| json!({"label": format!("L{n}"), "value": "v"})).collect();
        let card = |title: Value, tone: Value, fields: Value| {
            json!({"kind": "agent_card", "version": 1, "title": title, "tone": tone, "fields": fields})
        };
        let cases: Vec<(&str, Value, u32)> = vec![
            ("a tone outside the closed set", card(json!("T"), json!("urgent"), json!([])), 1),
            ("a tone in capitals", card(json!("T"), json!("INFO"), json!([])), 1),
            ("a blank title", card(json!("   "), json!("info"), json!([])), 1),
            ("a title with a link", card(json!("see https://example.com"), json!("info"), json!([])), 1),
            ("a title with a control character", card(json!("a\u{7}b"), json!("info"), json!([])), 1),
            ("a title over 120 characters", card(json!("t".repeat(CARD_TITLE_MAX_CHARS + 1)), json!("info"), json!([])), 1),
            ("a title that is not text", card(json!(7), json!("info"), json!([])), 1),
            ("fields that are not an array", card(json!("T"), json!("info"), json!("x")), 1),
            ("nine fields", card(json!("T"), json!("info"), json!(nine)), 1),
            ("a field without a value", card(json!("T"), json!("info"), json!([{"label": "L"}])), 1),
            ("a field with a blank label", card(json!("T"), json!("info"), json!([{"label": " ", "value": "v"}])), 1),
            (
                "a field label over 40 characters",
                card(json!("T"), json!("info"), json!([{"label": "l".repeat(CARD_FIELD_LABEL_MAX_CHARS + 1), "value": "v"}])),
                1,
            ),
            (
                "a field value over 200 characters",
                card(json!("T"), json!("info"), json!([{"label": "L", "value": "v".repeat(CARD_FIELD_VALUE_MAX_CHARS + 1)}])),
                1,
            ),
            (
                "a field value with a link",
                card(json!("T"), json!("info"), json!([{"label": "L", "value": "www.example.com"}])),
                1,
            ),
            ("a field that is not an object", card(json!("T"), json!("info"), json!(["x"])), 1),
            ("no title at all", json!({"kind": "agent_card", "version": 1, "tone": "info", "fields": []}), 1),
            ("no tone at all", json!({"kind": "agent_card", "version": 1, "title": "T", "fields": []}), 1),
            ("no fields at all", json!({"kind": "agent_card", "version": 1, "title": "T", "tone": "info"}), 1),
            (
                "a newer version carries no payload",
                json!({"kind": "agent_card", "version": 2, "title": "T", "tone": "info", "fields": []}),
                2,
            ),
            (
                "version zero carries no payload",
                json!({"kind": "agent_card", "version": 0, "title": "T", "tone": "info", "fields": []}),
                0,
            ),
        ];
        for (why, stored, version) in cases {
            let read = entry_from_stored(&stored).unwrap_or_else(|| panic!("{why}: the kind and version are kept"));
            assert_eq!((read.kind.as_str(), read.version), ("agent_card", version), "{why}");
            assert!(read.agent_card.is_none(), "{why}: no payload");
            assert!(read.link_previews.is_empty(), "{why}");
        }
    }
}
