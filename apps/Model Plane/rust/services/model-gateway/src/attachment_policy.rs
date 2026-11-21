//! The server's contract for what a chat turn may attach (WS08-S2).
//!
//! The browser used to be the only place the caps lived, so a request that skipped
//! it reached the model with whatever it carried: an image on a subscription
//! route was analysed on the platform chain, a document kind was dropped without
//! a word and bad base64 quietly turned the turn into text only. This module is
//! the one place that decides, before any model call, whether every attachment is
//! one this server will really use, and says why in a closed set of codes when it
//! is not. The same numbers are published beside `/v1/models` so the client
//! declares them instead of mirroring them.
//!
//! The policy only validates and declares: it never reads, stores or logs the
//! bytes it measures, and zero-data-retention turns never reach it (the ZDR
//! refusal of an attachment comes first, from `zdr_features`).

use base64::Engine as _;
use serde::Serialize;

use crate::vision::{parts::ENABLED_VAR as VISION_AS_PARTS_VAR, InvokeAttachment};

/// Opt-out switch. On unless the operator writes `0` or `false`: a typo keeps the
/// contract enforced, and rolling the contract back is a deliberate act.
pub const ENABLED_VAR: &str = "CHAT_ATTACHMENT_POLICY_ENABLED";
/// Opt-in switch of the sandbox staging of attached files (WS08-S5). Declared
/// here so the policy publishes what the server can really do with a file.
pub const SANDBOX_FILES_VAR: &str = "CHAT_SANDBOX_ATTACHMENTS_ENABLED";
/// Opt-in switch that lets a Do turn carry its attachments (WS08-S6), declared as
/// `modes.do_attachments`. `sse.rs` reads the same variable under its private
/// `DO_ATTACHMENTS_VAR`, which must equal this value; a test below pins that.
pub const DO_ATTACHMENTS_VAR: &str = "CHAT_DO_ATTACHMENTS_ENABLED";
const MAX_IMAGES_VAR: &str = "CHAT_ATTACHMENT_MAX_IMAGES";
const MAX_IMAGE_BYTES_VAR: &str = "CHAT_ATTACHMENT_MAX_IMAGE_BYTES";
const MAX_FILES_VAR: &str = "CHAT_ATTACHMENT_MAX_FILES";
const MAX_FILE_BYTES_VAR: &str = "CHAT_ATTACHMENT_MAX_FILE_BYTES";
const MAX_TOTAL_BYTES_VAR: &str = "CHAT_ATTACHMENT_MAX_TOTAL_BYTES";
/// Operator allowlist of the hosts a hosted image (a `url` with no bytes) may name. Unset or empty
/// keeps every hosted address as it was. Set, the address must be https, carry no userinfo and name
/// a host on the list: a lower-case host name, or `*.` and a domain for any subdomain of it. The
/// entries are comma-separated, and an entry that is not of that shape is dropped.
pub const HOSTED_IMAGE_HOSTS_VAR: &str = "CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS";
/// Request body limit of the chat stream route.
pub const STREAM_BODY_LIMIT_VAR: &str = "CHAT_STREAM_BODY_LIMIT_BYTES";

const DEFAULT_MAX_IMAGES: usize = 4;
const DEFAULT_MAX_IMAGE_BYTES: usize = 1_000_000;
const DEFAULT_MAX_FILES: usize = 3;
const DEFAULT_MAX_FILE_BYTES: usize = 1_000_000;
const DEFAULT_MAX_TOTAL_BYTES: usize = 3_145_728;
/// Room for [`DEFAULT_MAX_TOTAL_BYTES`] as base64 plus the conversation text, so
/// the declared caps are reachable instead of failing as a bare 413 in a hop.
pub const DEFAULT_STREAM_BODY_LIMIT_BYTES: usize = 12 * 1024 * 1024;

/// The image types a vision route accepts.
pub const IMAGE_MIMES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const MAX_NAME_CHARS: usize = 255;

/// What the stream route's body limit is: the env value when it is a positive
/// number, otherwise [`DEFAULT_STREAM_BODY_LIMIT_BYTES`].
#[must_use]
pub fn stream_body_limit_bytes() -> usize {
    stream_body_limit_with(&|key| std::env::var(key).ok())
}

fn stream_body_limit_with(get: &dyn Fn(&str) -> Option<String>) -> usize {
    positive(get, STREAM_BODY_LIMIT_VAR, DEFAULT_STREAM_BODY_LIMIT_BYTES)
}

fn positive(get: &dyn Fn(&str) -> Option<String>, key: &str, default: usize) -> usize {
    get(key)
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// The closed set of reasons an attachment is refused. Each is a stable code a
/// client matches on, never a free string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentCode {
    MediaUnsupported,
    TooLarge,
    CountExceeded,
    RouteUnsupported,
    EncodingInvalid,
}

impl AttachmentCode {
    pub const ALL: [Self; 5] = [
        Self::MediaUnsupported,
        Self::TooLarge,
        Self::CountExceeded,
        Self::RouteUnsupported,
        Self::EncodingInvalid,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MediaUnsupported => "attachment_media_unsupported",
            Self::TooLarge => "attachment_too_large",
            Self::CountExceeded => "attachment_count_exceeded",
            Self::RouteUnsupported => "attachment_route_unsupported",
            Self::EncodingInvalid => "attachment_encoding_invalid",
        }
    }
}

/// One refused attachment: the code and a sentence for the person, in English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentRefusal {
    pub code: AttachmentCode,
    pub message: String,
}

impl AttachmentRefusal {
    fn new(code: AttachmentCode, message: String) -> Self {
        Self { code, message }
    }
}

/// The route the turn will be served on, as far as attachments care: the user's
/// own `ChatGPT` subscription never reads attachments, so a turn on it must say so
/// instead of being analysed on the platform chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentRoute {
    Platform,
    Subscription,
}

impl AttachmentRoute {
    /// The route a request names. Read from the raw body because the turn's
    /// normalisation refuses a subscription turn with attachments in a generic
    /// code, which would hide this one.
    #[must_use]
    pub fn named(provider: Option<&str>, subscription_connection_id: Option<&str>) -> Self {
        let pinned = provider.is_some_and(crate::normalize::is_codex_subscription_hint)
            || subscription_connection_id.is_some_and(|id| !id.trim().is_empty());
        if pinned {
            Self::Subscription
        } else {
            Self::Platform
        }
    }
}

/// The contract's numbers and switches as the environment sets them. Not `Copy`: it owns the
/// hosted-image host list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentPolicy {
    enabled: bool,
    max_images: usize,
    max_image_bytes: usize,
    max_files: usize,
    max_file_bytes: usize,
    max_total_bytes: usize,
    sandbox_files: bool,
    vision_as_parts: bool,
    do_attachments: bool,
    /// The hosts a hosted image may name. `None` when the list is unset, which allows every hosted address.
    hosted_image_hosts: Option<Vec<String>>,
}

impl Default for AttachmentPolicy {
    fn default() -> Self {
        Self::from_getter(&|_| None)
    }
}

impl AttachmentPolicy {
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_getter(&|key| std::env::var(key).ok())
    }

    /// Reads every value through `get`, so a test never touches the process
    /// environment. A missing, zero or unparsable number keeps its default.
    #[must_use]
    pub fn from_getter(get: &dyn Fn(&str) -> Option<String>) -> Self {
        let enabled = !get(ENABLED_VAR).is_some_and(|value| {
            let value = value.trim();
            value == "0" || value.eq_ignore_ascii_case("false")
        });
        Self {
            enabled,
            max_images: positive(get, MAX_IMAGES_VAR, DEFAULT_MAX_IMAGES),
            max_image_bytes: positive(get, MAX_IMAGE_BYTES_VAR, DEFAULT_MAX_IMAGE_BYTES),
            max_files: positive(get, MAX_FILES_VAR, DEFAULT_MAX_FILES),
            max_file_bytes: positive(get, MAX_FILE_BYTES_VAR, DEFAULT_MAX_FILE_BYTES),
            max_total_bytes: positive(get, MAX_TOTAL_BYTES_VAR, DEFAULT_MAX_TOTAL_BYTES),
            sandbox_files: mp_contracts::loop_fold::switch_from(get(SANDBOX_FILES_VAR).as_deref()),
            vision_as_parts: mp_contracts::loop_fold::switch_from(
                get(VISION_AS_PARTS_VAR).as_deref(),
            ),
            do_attachments: mp_contracts::loop_fold::switch_from(
                get(DO_ATTACHMENTS_VAR).as_deref(),
            ),
            hosted_image_hosts: hosted_image_hosts_from(get(HOSTED_IMAGE_HOSTS_VAR).as_deref()),
        }
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Checks every attachment of one turn, before any model call. The first
    /// problem wins and nothing is dropped: an attachment this server cannot use
    /// is refused, not skipped. With the switch off it accepts everything, which
    /// is the behaviour before this contract existed.
    ///
    /// # Errors
    /// The typed refusal for the first attachment (or the whole set) that breaks
    /// the policy.
    pub fn validate(
        &self,
        attachments: &[InvokeAttachment],
        route: AttachmentRoute,
    ) -> Result<(), AttachmentRefusal> {
        if !self.enabled || attachments.is_empty() {
            return Ok(());
        }
        if route == AttachmentRoute::Subscription {
            return Err(AttachmentRefusal::new(
                AttachmentCode::RouteUnsupported,
                "Attachments are not read on your own ChatGPT subscription. Send the message without them, or pick a Verevon model."
                    .to_owned(),
            ));
        }
        let mut kinds = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            kinds.push(self.kind_of(attachment)?);
        }
        let images = kinds.iter().filter(|kind| **kind == Kind::Image).count();
        let files = kinds.len() - images;
        if images > self.max_images {
            return Err(AttachmentRefusal::new(
                AttachmentCode::CountExceeded,
                format!(
                    "At most {} images can be attached to one message ({images} were sent).",
                    self.max_images
                ),
            ));
        }
        if files > self.max_files {
            return Err(AttachmentRefusal::new(
                AttachmentCode::CountExceeded,
                format!(
                    "At most {} files can be attached to one message ({files} were sent).",
                    self.max_files
                ),
            ));
        }
        let mut total = 0_usize;
        for (attachment, kind) in attachments.iter().zip(&kinds) {
            let limit = match kind {
                Kind::Image => self.max_image_bytes,
                Kind::File => self.max_file_bytes,
            };
            total = total.saturating_add(self.measure(attachment, *kind, limit)?);
        }
        if total > self.max_total_bytes {
            return Err(AttachmentRefusal::new(
                AttachmentCode::TooLarge,
                format!(
                    "The attachments add up to {total} bytes; the limit for one message is {} bytes.",
                    self.max_total_bytes
                ),
            ));
        }
        Ok(())
    }

    /// The declared policy for `/v1/models`, or `None` when the contract is off
    /// (the client then keeps its own constants, as before). `vision` is whether
    /// inference-core lists a model that reads images.
    #[must_use]
    pub fn declared(&self, vision: bool) -> Option<DeclaredAttachmentPolicy> {
        self.enabled.then(|| DeclaredAttachmentPolicy {
            image_mimes: IMAGE_MIMES.to_vec(),
            max_images: self.max_images,
            max_image_bytes: self.max_image_bytes,
            max_files: self.max_files,
            max_file_bytes: self.max_file_bytes,
            max_total_bytes: self.max_total_bytes,
            modes: AttachmentModes {
                vision,
                sandbox_files: self.sandbox_files,
                vision_as_parts: self.vision_as_parts,
                do_attachments: self.do_attachments,
            },
        })
    }

    /// An image, or a file the sandbox can be given. Any other kind, and a file
    /// while no consumer for it is switched on, is refused rather than ignored.
    fn kind_of(&self, attachment: &InvokeAttachment) -> Result<Kind, AttachmentRefusal> {
        let label = label_of(attachment);
        let mime = normalized_mime(attachment.mime_type.as_deref());
        let kind = match attachment.kind.as_deref().map(str::trim) {
            Some(kind) if kind.eq_ignore_ascii_case("image") => Kind::Image,
            Some(kind) if kind.eq_ignore_ascii_case("file") => Kind::File,
            None if mime.starts_with("image/") => Kind::Image,
            other => {
                return Err(AttachmentRefusal::new(
                    AttachmentCode::MediaUnsupported,
                    format!(
                        "{label}: the attachment kind '{}' is not supported.",
                        other.unwrap_or("unspecified")
                    ),
                ));
            }
        };
        match kind {
            Kind::Image if !IMAGE_MIMES.contains(&mime.as_str()) => Err(AttachmentRefusal::new(
                AttachmentCode::MediaUnsupported,
                format!(
                    "{label}: images must be PNG, JPEG, WebP or GIF (got '{}').",
                    if mime.is_empty() { "no type" } else { &mime }
                ),
            )),
            Kind::File if !self.sandbox_files => Err(AttachmentRefusal::new(
                AttachmentCode::MediaUnsupported,
                format!("{label}: this server does not accept file attachments yet."),
            )),
            _ => Ok(kind),
        }
    }

    /// Whether a hosted image's address may reach the provider. Without the operator's list every
    /// address may; with it, only an address [`url_on_list`] accepts.
    fn hosted_url_allowed(&self, url: &str) -> bool {
        match &self.hosted_image_hosts {
            None => true,
            Some(hosts) => url_on_list(url, hosts),
        }
    }

    /// Whether an image address that a route forwards without a turn may reach the provider. The image
    /// analysis and OCR routes send a hosted address on, and the provider fetches it, so it meets the same
    /// list a hosted chat attachment meets in [`Self::validate`]. With the contract switched off, or no
    /// operator list, every address passes as it did before the contract existed.
    ///
    /// # Errors
    /// `EncodingInvalid` when the list is set and the address is not on it. The address is never echoed.
    pub fn check_hosted_image_url(&self, url: &str) -> Result<(), AttachmentRefusal> {
        if !self.enabled || self.hosted_url_allowed(url) {
            return Ok(());
        }
        Err(AttachmentRefusal::new(
            AttachmentCode::EncodingInvalid,
            "The image address is not allowed.".to_owned(),
        ))
    }

    /// Validates the name and the encoding of one attachment and returns its
    /// decoded size, refusing it when that is over `limit`. A hosted image
    /// (`url`, no bytes) has no size here; its address must pass the host list
    /// when one is set.
    fn measure(
        &self,
        attachment: &InvokeAttachment,
        kind: Kind,
        limit: usize,
    ) -> Result<usize, AttachmentRefusal> {
        let label = label_of(attachment);
        if let Some(name) = attachment.name.as_deref() {
            if let Err(why) = check_name(name) {
                return Err(AttachmentRefusal::new(
                    AttachmentCode::EncodingInvalid,
                    format!("{label}: the file name {why}."),
                ));
            }
        } else if kind == Kind::File {
            return Err(AttachmentRefusal::new(
                AttachmentCode::EncodingInvalid,
                "A file attachment needs a name.".to_owned(),
            ));
        }
        let data = attachment
            .data_base64
            .as_deref()
            .filter(|value| !value.is_empty());
        let Some(data) = data else {
            let url = attachment
                .url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty());
            return match url {
                Some(url) if kind == Kind::Image => {
                    if self.hosted_url_allowed(url) {
                        Ok(0)
                    } else {
                        // The address is never echoed: it can carry the person's data.
                        Err(AttachmentRefusal::new(
                            AttachmentCode::EncodingInvalid,
                            format!("{label}: the image address is not allowed."),
                        ))
                    }
                }
                _ => Err(AttachmentRefusal::new(
                    AttachmentCode::EncodingInvalid,
                    format!("{label}: the attachment carries no data."),
                )),
            };
        };
        // The same tolerance the vision path has: a `data:` URL prefix is dropped.
        let payload = data.rsplit(',').next().unwrap_or(data);
        let too_large = |size: String| {
            AttachmentRefusal::new(
                AttachmentCode::TooLarge,
                format!("{label}: {size}; the limit for one {} is {limit} bytes.", kind.noun()),
            )
        };
        // Refuse an oversized payload before spending memory decoding it.
        if payload.len() > limit.div_ceil(3) * 4 + 4 {
            return Err(too_large("it is larger than that".to_owned()));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|_| {
                AttachmentRefusal::new(
                    AttachmentCode::EncodingInvalid,
                    format!("{label}: the attachment data is not valid base64."),
                )
            })?;
        if bytes.is_empty() {
            return Err(AttachmentRefusal::new(
                AttachmentCode::EncodingInvalid,
                format!("{label}: the attachment is empty."),
            ));
        }
        if bytes.len() > limit {
            return Err(too_large(format!("it is {} bytes", bytes.len())));
        }
        Ok(bytes.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Image,
    File,
}

impl Kind {
    const fn noun(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::File => "file",
        }
    }
}

/// How the person is told which attachment is meant: its name when it has a safe
/// one, otherwise a neutral word. Never the data.
fn label_of(attachment: &InvokeAttachment) -> String {
    match attachment.name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() && check_name(name).is_ok() => format!("'{name}'"),
        _ => "An attachment".to_owned(),
    }
}

fn normalized_mime(mime: Option<&str>) -> String {
    mime.unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// A bare file name or nothing: the name reaches the sandbox workspace later, so
/// a name that could address a second directory is refused, never repaired.
fn check_name(raw: &str) -> Result<(), &'static str> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("is empty");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err("is too long");
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err("must not be a path");
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err("must not contain a path separator or '..'");
    }
    if name.chars().any(char::is_control) {
        return Err("contains a control character");
    }
    Ok(())
}

/// The hosts [`HOSTED_IMAGE_HOSTS_VAR`] names, or `None` when it is unset or empty. Entries are
/// comma-separated and surrounding blanks are ignored. An entry that is not a lower-case host name
/// (or `*.` and one) is dropped, with one warning per process, and a list left empty allows no host.
fn hosted_image_hosts_from(raw: Option<&str>) -> Option<Vec<String>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut hosts = Vec::new();
    let mut dropped = 0_usize;
    let entries = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty());
    for entry in entries {
        if is_host_entry(entry) {
            hosts.push(entry.to_owned());
        } else {
            dropped += 1;
        }
    }
    if dropped > 0 {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            tracing::warn!(
                dropped,
                "CHAT_ATTACHMENT_HOSTED_IMAGE_HOSTS: entries that are not lower-case host names were dropped"
            );
        });
    }
    Some(hosts)
}

/// A lower-case host name, or `*.` and one, of letters, digits, `.` and `-` only.
fn is_host_entry(entry: &str) -> bool {
    let name = entry.strip_prefix("*.").unwrap_or(entry);
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}

/// Whether `raw` is an address the operator's list lets the provider fetch: https, no userinfo, and
/// a host name (never an IP literal, never an empty label) that is an entry or sits below a `*.`
/// entry. The address is parsed here and never fetched, and it is never put in a message or a log.
fn url_on_list(raw: &str, hosts: &[String]) -> bool {
    // The URL standard reads a backslash as a path separator, another parser as part of the authority:
    // `https://cdn.example.org\@10.0.0.1/a.png` would name the listed host here and an internal one there.
    // A blank, a control character and a non-ASCII byte are refused for the same reason, before the parse.
    if raw
        .bytes()
        .any(|byte| byte <= b' ' || byte == b'\\' || byte >= 0x7f)
    {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    if host.starts_with('[')
        || host.parse::<std::net::IpAddr>().is_ok()
        || host.split('.').any(str::is_empty)
    {
        return false;
    }
    hosts.iter().any(|entry| match entry.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .and_then(|rest| rest.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty()),
        None => host == *entry,
    })
}

/// `attachment_policy` of `/v1/models`: the numbers the server enforces, so the
/// client declares them instead of keeping its own copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeclaredAttachmentPolicy {
    pub image_mimes: Vec<&'static str>,
    pub max_images: usize,
    pub max_image_bytes: usize,
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
    pub modes: AttachmentModes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AttachmentModes {
    /// A model that reads images is listed by inference-core.
    pub vision: bool,
    /// An attached file is staged read-only into the code sandbox (WS08-S5).
    pub sandbox_files: bool,
    /// Images ride to the model as parts of an Ask turn (WS08-S4, `CHAT_VISION_AS_PARTS_ENABLED`)
    /// instead of the vision diversion. While a policy is declared the client gates images on this
    /// value rather than on its own build flag, so the two cannot drift apart.
    pub vision_as_parts: bool,
    /// A Do run carries the turn's attachments (`CHAT_DO_ATTACHMENTS_ENABLED`). While a policy is
    /// declared, the client gates images on a Do turn on this value.
    pub do_attachments: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // "hi" and "hello".
    const TINY: &str = "aGk=";
    const HELLO: &str = "aGVsbG8=";

    fn policy(vars: &[(&str, &str)]) -> AttachmentPolicy {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        AttachmentPolicy::from_getter(&move |key| {
            vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        })
    }

    fn image(data: &str) -> InvokeAttachment {
        InvokeAttachment {
            kind: Some("image".to_owned()),
            data_base64: Some(data.to_owned()),
            mime_type: Some("image/png".to_owned()),
            ..Default::default()
        }
    }

    fn file(name: &str, data: &str) -> InvokeAttachment {
        InvokeAttachment {
            kind: Some("file".to_owned()),
            name: Some(name.to_owned()),
            data_base64: Some(data.to_owned()),
            mime_type: Some("text/csv".to_owned()),
            ..Default::default()
        }
    }

    fn code_of(result: Result<(), AttachmentRefusal>) -> AttachmentCode {
        result.expect_err("the attachments were refused").code
    }

    #[test]
    fn subscription_route_with_any_attachment_is_refused() {
        let refusal = AttachmentPolicy::default()
            .validate(&[image(TINY)], AttachmentRoute::Subscription)
            .expect_err("refused");
        assert_eq!(refusal.code, AttachmentCode::RouteUnsupported);
        assert_eq!(refusal.code.as_str(), "attachment_route_unsupported");
        // Even an attachment that would be refused for another reason names the route.
        let unknown = InvokeAttachment {
            kind: Some("document".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            code_of(AttachmentPolicy::default().validate(&[unknown], AttachmentRoute::Subscription)),
            AttachmentCode::RouteUnsupported
        );
    }

    #[test]
    fn a_subscription_turn_without_attachments_is_untouched() {
        assert!(AttachmentPolicy::default()
            .validate(&[], AttachmentRoute::Subscription)
            .is_ok());
    }

    #[test]
    fn the_route_is_read_from_the_provider_and_the_connection() {
        let named = AttachmentRoute::named;
        assert_eq!(named(None, None), AttachmentRoute::Platform);
        assert_eq!(named(Some("azure-openai"), None), AttachmentRoute::Platform);
        assert_eq!(named(Some(" "), Some(" ")), AttachmentRoute::Platform);
        for provider in ["openai-codex-subscription", "codex-subscription", "chatgpt-codex"] {
            assert_eq!(named(Some(provider), None), AttachmentRoute::Subscription, "{provider}");
        }
        assert_eq!(named(None, Some("conn_1")), AttachmentRoute::Subscription);
    }

    #[test]
    fn fifth_image_is_refused() {
        let four = vec![image(TINY); 4];
        assert!(AttachmentPolicy::default()
            .validate(&four, AttachmentRoute::Platform)
            .is_ok());
        let five = vec![image(TINY); 5];
        let refusal = AttachmentPolicy::default()
            .validate(&five, AttachmentRoute::Platform)
            .expect_err("refused");
        assert_eq!(refusal.code, AttachmentCode::CountExceeded);
        assert!(refusal.message.contains('4'), "{}", refusal.message);
    }

    #[test]
    fn oversize_image_names_limit() {
        let policy = policy(&[(MAX_IMAGE_BYTES_VAR, "4")]);
        let mut big = image(HELLO);
        big.name = Some("chart.png".to_owned());
        let refusal = policy
            .validate(&[big], AttachmentRoute::Platform)
            .expect_err("refused");
        assert_eq!(refusal.code, AttachmentCode::TooLarge);
        assert!(refusal.message.contains("'chart.png'"), "{}", refusal.message);
        assert!(refusal.message.contains("limit"), "{}", refusal.message);
        assert!(refusal.message.contains('4'), "{}", refusal.message);
    }

    #[test]
    fn an_image_at_the_limit_passes() {
        assert!(policy(&[(MAX_IMAGE_BYTES_VAR, "5")])
            .validate(&[image(HELLO)], AttachmentRoute::Platform)
            .is_ok());
    }

    #[test]
    fn the_total_is_capped_across_attachments() {
        let policy = policy(&[(MAX_TOTAL_BYTES_VAR, "9")]);
        assert_eq!(
            code_of(policy.validate(&[image(HELLO), image(HELLO)], AttachmentRoute::Platform)),
            AttachmentCode::TooLarge
        );
        assert!(policy
            .validate(&[image(HELLO), image(TINY)], AttachmentRoute::Platform)
            .is_ok());
    }

    #[test]
    fn an_absurd_payload_is_refused_without_being_decoded() {
        let huge = "A".repeat(DEFAULT_MAX_IMAGE_BYTES * 2);
        assert_eq!(
            code_of(AttachmentPolicy::default().validate(&[image(&huge)], AttachmentRoute::Platform)),
            AttachmentCode::TooLarge
        );
    }

    #[test]
    fn unknown_kind_is_refused_not_ignored() {
        for kind in [Some("document"), Some("audio"), None] {
            let attachment = InvokeAttachment {
                kind: kind.map(str::to_owned),
                data_base64: Some(TINY.to_owned()),
                mime_type: Some("application/pdf".to_owned()),
                ..Default::default()
            };
            assert_eq!(
                code_of(AttachmentPolicy::default().validate(&[attachment], AttachmentRoute::Platform)),
                AttachmentCode::MediaUnsupported,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn an_image_type_outside_the_list_is_refused() {
        let mut svg = image(TINY);
        svg.mime_type = Some("image/svg+xml".to_owned());
        assert_eq!(
            code_of(AttachmentPolicy::default().validate(&[svg], AttachmentRoute::Platform)),
            AttachmentCode::MediaUnsupported
        );
        let mut untyped = image(TINY);
        untyped.mime_type = None;
        assert_eq!(
            code_of(AttachmentPolicy::default().validate(&[untyped], AttachmentRoute::Platform)),
            AttachmentCode::MediaUnsupported
        );
    }

    #[test]
    fn every_listed_image_type_passes_with_parameters_and_case() {
        for mime in ["image/png", "IMAGE/JPEG", "image/webp", "image/gif; charset=binary"] {
            let mut attachment = image(TINY);
            attachment.mime_type = Some(mime.to_owned());
            assert!(
                AttachmentPolicy::default()
                    .validate(&[attachment], AttachmentRoute::Platform)
                    .is_ok(),
                "{mime}"
            );
        }
    }

    #[test]
    fn bad_base64_is_refused_not_skipped() {
        for data in ["!!!not base64!!!", "aGk"] {
            assert_eq!(
                code_of(AttachmentPolicy::default().validate(&[image(data)], AttachmentRoute::Platform)),
                AttachmentCode::EncodingInvalid,
                "{data}"
            );
        }
    }

    #[test]
    fn an_attachment_with_no_data_is_refused_unless_it_is_a_hosted_image() {
        let empty = InvokeAttachment {
            kind: Some("image".to_owned()),
            mime_type: Some("image/png".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            code_of(AttachmentPolicy::default().validate(&[empty.clone()], AttachmentRoute::Platform)),
            AttachmentCode::EncodingInvalid
        );
        let hosted = InvokeAttachment {
            url: Some("https://example.org/a.png".to_owned()),
            ..empty
        };
        assert!(AttachmentPolicy::default()
            .validate(&[hosted], AttachmentRoute::Platform)
            .is_ok());
    }

    #[test]
    fn a_data_url_prefix_is_tolerated_as_the_vision_path_does() {
        assert!(AttachmentPolicy::default()
            .validate(
                &[image("data:image/png;base64,aGk=")],
                AttachmentRoute::Platform
            )
            .is_ok());
    }

    #[test]
    fn policy_off_restores_current_behaviour() {
        let off = policy(&[(ENABLED_VAR, "0")]);
        assert!(!off.enabled());
        let hopeless = vec![
            image("!!!"),
            InvokeAttachment {
                kind: Some("document".to_owned()),
                ..Default::default()
            },
        ];
        assert!(off.validate(&hopeless, AttachmentRoute::Platform).is_ok());
        assert!(off.validate(&hopeless, AttachmentRoute::Subscription).is_ok());
        assert_eq!(off.declared(true), None, "an off contract declares nothing");
        assert!(policy(&[(ENABLED_VAR, "false")]).declared(true).is_none());
    }

    #[test]
    fn only_zero_or_false_switches_the_contract_off() {
        assert!(AttachmentPolicy::default().enabled());
        for value in ["", "1", "true", "off", "no", "garbage"] {
            assert!(policy(&[(ENABLED_VAR, value)]).enabled(), "{value:?}");
        }
    }

    #[test]
    fn a_file_is_refused_until_the_sandbox_switch_is_on() {
        assert_eq!(
            code_of(AttachmentPolicy::default().validate(
                &[file("sales.csv", TINY)],
                AttachmentRoute::Platform
            )),
            AttachmentCode::MediaUnsupported
        );
        let staged = policy(&[(SANDBOX_FILES_VAR, "true")]);
        assert!(staged
            .validate(&[file("sales.csv", TINY)], AttachmentRoute::Platform)
            .is_ok());
        assert_eq!(
            code_of(policy(&[(SANDBOX_FILES_VAR, "1")]).validate(
                &[file("sales.csv", TINY)],
                AttachmentRoute::Platform
            )),
            AttachmentCode::MediaUnsupported,
            "only the literal true switches the sandbox on"
        );
    }

    #[test]
    fn file_count_size_and_name_are_enforced() {
        let staged = policy(&[(SANDBOX_FILES_VAR, "true"), (MAX_FILE_BYTES_VAR, "4")]);
        let four_files: Vec<_> = (0..4).map(|n| file(&format!("f{n}.csv"), TINY)).collect();
        assert_eq!(
            code_of(staged.validate(&four_files, AttachmentRoute::Platform)),
            AttachmentCode::CountExceeded
        );
        assert_eq!(
            code_of(staged.validate(&[file("big.csv", HELLO)], AttachmentRoute::Platform)),
            AttachmentCode::TooLarge
        );
        for name in ["../x.csv", "a/b.csv", "a\\b.csv", "/etc/passwd", "", "bad\nname"] {
            assert_eq!(
                code_of(staged.validate(&[file(name, TINY)], AttachmentRoute::Platform)),
                AttachmentCode::EncodingInvalid,
                "{name:?}"
            );
        }
        let mut nameless = file("x.csv", TINY);
        nameless.name = None;
        assert_eq!(
            code_of(staged.validate(&[nameless], AttachmentRoute::Platform)),
            AttachmentCode::EncodingInvalid
        );
    }

    #[test]
    fn an_unsafe_name_never_reaches_the_message() {
        let mut attachment = image(TINY);
        attachment.name = Some("../../secret\nname.png".to_owned());
        let refusal = AttachmentPolicy::default()
            .validate(&[attachment], AttachmentRoute::Platform)
            .expect_err("refused");
        assert!(!refusal.message.contains("secret"), "{}", refusal.message);
    }

    #[test]
    fn env_numbers_replace_defaults_and_bad_ones_keep_them() {
        let declared = policy(&[
            (MAX_IMAGES_VAR, "2"),
            (MAX_FILES_VAR, "0"),
            (MAX_TOTAL_BYTES_VAR, "many"),
            (MAX_IMAGE_BYTES_VAR, " 2048 "),
        ])
        .declared(true)
        .expect("declared");
        assert_eq!(declared.max_images, 2);
        assert_eq!(declared.max_files, DEFAULT_MAX_FILES);
        assert_eq!(declared.max_total_bytes, DEFAULT_MAX_TOTAL_BYTES);
        assert_eq!(declared.max_image_bytes, 2048);
    }

    #[test]
    fn the_declared_policy_is_the_documented_json() {
        let declared = AttachmentPolicy::default().declared(true).expect("declared");
        assert_eq!(
            serde_json::to_value(&declared).expect("serialises"),
            serde_json::json!({
                "image_mimes": ["image/png", "image/jpeg", "image/webp", "image/gif"],
                "max_images": 4,
                "max_image_bytes": 1_000_000,
                "max_files": 3,
                "max_file_bytes": 1_000_000,
                "max_total_bytes": 3_145_728,
                "modes": {
                    "vision": true,
                    "sandbox_files": false,
                    "vision_as_parts": false,
                    "do_attachments": false,
                },
            })
        );
        assert!(!AttachmentPolicy::default().declared(false).expect("declared").modes.vision);
    }

    #[test]
    fn the_declared_modes_follow_each_switch_and_only_the_literal_true_enables_it() {
        let modes = |vars: &[(&str, &str)]| policy(vars).declared(true).expect("declared").modes;
        let none = modes(&[]);
        assert!(!none.vision_as_parts && !none.do_attachments);
        let parts = modes(&[(VISION_AS_PARTS_VAR, "true")]);
        assert!(parts.vision_as_parts && !parts.do_attachments);
        let do_run = modes(&[(DO_ATTACHMENTS_VAR, "true")]);
        assert!(do_run.do_attachments && !do_run.vision_as_parts);
        assert!(
            !modes(&[(DO_ATTACHMENTS_VAR, " true ")]).do_attachments,
            "a padded value is off: the reader is exact"
        );
        for value in ["1", "yes", "on", "TRUE", ""] {
            assert!(
                !modes(&[(VISION_AS_PARTS_VAR, value)]).vision_as_parts,
                "{value:?}"
            );
            assert!(
                !modes(&[(DO_ATTACHMENTS_VAR, value)]).do_attachments,
                "{value:?}"
            );
        }
        let sandbox = modes(&[(SANDBOX_FILES_VAR, "true")]);
        assert!(sandbox.sandbox_files && !sandbox.vision_as_parts && !sandbox.do_attachments);
    }

    #[test]
    fn the_switch_names_are_the_ones_the_operator_sets_and_sse_reads() {
        assert_eq!(VISION_AS_PARTS_VAR, "CHAT_VISION_AS_PARTS_ENABLED");
        assert_eq!(DO_ATTACHMENTS_VAR, "CHAT_DO_ATTACHMENTS_ENABLED");
        // `sse.rs` keeps a private copy of the Do name. A declared `do_attachments` that
        // disagreed with it would promise attachments the stream never carries.
        let sse = include_str!("sse.rs");
        assert!(
            sse.contains(&format!(
                "const DO_ATTACHMENTS_VAR: &str = \"{DO_ATTACHMENTS_VAR}\";"
            )),
            "sse.rs must read the Do switch under {DO_ATTACHMENTS_VAR}"
        );
    }

    #[test]
    fn the_body_limit_holds_the_declared_total_as_base64() {
        let none = |_: &str| None;
        assert_eq!(stream_body_limit_with(&none), 12 * 1024 * 1024);
        assert!(
            DEFAULT_STREAM_BODY_LIMIT_BYTES > DEFAULT_MAX_TOTAL_BYTES.div_ceil(3) * 4,
            "the caps must be reachable inside the body limit"
        );
        let set = |_: &str| Some("20971520".to_owned());
        assert_eq!(stream_body_limit_with(&set), 20_971_520);
        let bad = |_: &str| Some("0".to_owned());
        assert_eq!(stream_body_limit_with(&bad), DEFAULT_STREAM_BODY_LIMIT_BYTES);
    }

    #[test]
    fn the_codes_are_the_five_the_plan_names() {
        let codes: Vec<&str> = AttachmentCode::ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "attachment_media_unsupported",
                "attachment_too_large",
                "attachment_count_exceeded",
                "attachment_route_unsupported",
                "attachment_encoding_invalid",
            ]
        );
    }

    /// A hosted image: a `url` and no bytes.
    fn hosted(url: &str) -> InvokeAttachment {
        InvokeAttachment {
            kind: Some("image".to_owned()),
            mime_type: Some("image/png".to_owned()),
            url: Some(url.to_owned()),
            ..Default::default()
        }
    }

    /// Whether a hosted image at `url` passes when the operator lists `hosts`.
    fn hosted_passes(hosts: &str, url: &str) -> bool {
        policy(&[(HOSTED_IMAGE_HOSTS_VAR, hosts)])
            .validate(&[hosted(url)], AttachmentRoute::Platform)
            .is_ok()
    }

    #[test]
    fn a_listed_https_host_passes_and_the_other_hosted_forms_are_refused() {
        let list = "cdn.example.org";
        assert!(hosted_passes(list, "https://cdn.example.org/a.png"));
        // The scheme and the host are case-insensitive, as they are in a URL.
        assert!(hosted_passes(list, "HTTPS://CDN.Example.ORG/a.png"));
        for url in [
            "http://cdn.example.org/a.png",
            "HTTP://cdn.example.org/a.png",
            "https://user:secret@cdn.example.org/a.png",
            "https://user@cdn.example.org/a.png",
            "https://evil.example.net/a.png",
            "https://cdn.example.org./a.png",
            "https://cdn..example.org/a.png",
            "not a url",
        ] {
            assert!(!hosted_passes(list, url), "{url}");
        }
    }

    #[test]
    fn a_look_alike_host_is_not_the_listed_one() {
        let list = "example.org";
        assert!(hosted_passes(list, "https://example.org/a.png"));
        assert!(
            !hosted_passes(list, "https://xn--exmple-cua.org/a.png"),
            "a punycode look-alike"
        );
        assert!(
            !hosted_passes(list, "https://ex\u{e4}mple.org/a.png"),
            "an accented look-alike"
        );
    }

    #[test]
    fn an_uppercase_entry_is_not_a_lower_case_host_name_and_allows_nothing() {
        assert_eq!(
            hosted_image_hosts_from(Some("CDN.example.org")),
            Some(Vec::new())
        );
        assert!(!hosted_passes(
            "CDN.example.org",
            "https://cdn.example.org/a.png"
        ));
    }

    #[test]
    fn an_ip_literal_is_refused_even_when_the_list_names_it() {
        assert!(!hosted_passes("10.0.0.1", "https://10.0.0.1/a.png"));
        assert!(!hosted_passes("10.0.0.1", "https://0x0a000001/a.png"));
        assert!(!hosted_passes("cdn.example.org", "https://127.0.0.1/a.png"));
        assert!(!hosted_passes("cdn.example.org", "https://[::1]/a.png"));
    }

    #[test]
    fn a_wildcard_entry_matches_subdomains_on_a_label_boundary() {
        let list = "*.example.com";
        assert!(hosted_passes(list, "https://a.example.com/a.png"));
        assert!(hosted_passes(list, "https://deep.a.example.com/a.png"));
        for url in [
            "https://example.com/a.png",
            "https://evilexample.com/a.png",
            "https://a.evilexample.com/a.png",
            "https://example.com.evil.net/a.png",
        ] {
            assert!(!hosted_passes(list, url), "{url}");
        }
    }

    #[test]
    fn malformed_entries_are_dropped_and_an_emptied_list_allows_no_host() {
        assert_eq!(
            hosted_image_hosts_from(Some(" bad host, cdn.example.org ,,*.example.com, !x, *.")),
            Some(vec![
                "cdn.example.org".to_owned(),
                "*.example.com".to_owned()
            ])
        );
        assert!(hosted_passes(
            "bad host,cdn.example.org",
            "https://cdn.example.org/a.png"
        ));
        assert!(!hosted_passes("!!!", "https://cdn.example.org/a.png"));
        assert!(!hosted_passes(" , ", "https://cdn.example.org/a.png"));
    }

    #[test]
    fn an_unset_or_blank_list_changes_nothing() {
        assert_eq!(hosted_image_hosts_from(None), None);
        assert_eq!(hosted_image_hosts_from(Some("  ")), None);
        assert_eq!(AttachmentPolicy::default().hosted_image_hosts, None);
        for url in [
            "http://anywhere.test/a.png",
            "https://10.0.0.1/a.png",
            "ftp://x.test/a.png",
        ] {
            assert!(hosted_passes("", url), "{url}");
            assert!(AttachmentPolicy::default()
                .validate(&[hosted(url)], AttachmentRoute::Platform)
                .is_ok());
        }
    }

    #[test]
    fn the_list_judges_hosted_addresses_and_never_inline_bytes() {
        assert!(policy(&[(HOSTED_IMAGE_HOSTS_VAR, "cdn.example.org")])
            .validate(&[image(TINY)], AttachmentRoute::Platform)
            .is_ok());
    }

    #[test]
    fn a_refused_hosted_address_is_typed_and_never_echoed() {
        let refusal = policy(&[(HOSTED_IMAGE_HOSTS_VAR, "cdn.example.org")])
            .validate(
                &[hosted("https://evil.example.net/secret-chart.png")],
                AttachmentRoute::Platform,
            )
            .expect_err("refused");
        assert_eq!(refusal.code, AttachmentCode::EncodingInvalid);
        assert_eq!(
            refusal.message,
            "An attachment: the image address is not allowed."
        );
        assert!(
            !refusal.message.contains("evil.example.net"),
            "{}",
            refusal.message
        );
        assert!(!refusal.message.contains("secret"), "{}", refusal.message);
    }

    /// The image routes send a hosted address on without a turn, so they meet the same list through
    /// `check_hosted_image_url`.
    fn image_route_passes(vars: &[(&str, &str)], url: &str) -> bool {
        policy(vars).check_hosted_image_url(url).is_ok()
    }

    #[test]
    fn the_image_routes_judge_a_hosted_address_by_the_same_list() {
        let list = [(HOSTED_IMAGE_HOSTS_VAR, "cdn.example.org")];
        assert!(image_route_passes(&list, "https://cdn.example.org/a.png"));
        for url in [
            "http://cdn.example.org/a.png",
            "https://user:secret@cdn.example.org/a.png",
            "https://10.0.0.1/a.png",
            "https://127.0.0.1/a.png",
            "https://[::1]/a.png",
            "https://localhost/a.png",
            "https://evil.example.net/a.png",
            "https://cdn.example.org.evil.net/a.png",
            "https://cdn.example.org\\@10.0.0.1/a.png",
            "https://cdn.example.org/a b.png",
            " https://cdn.example.org/a.png",
            "not a url",
        ] {
            assert!(!image_route_passes(&list, url), "{url:?}");
        }
    }

    #[test]
    fn with_no_list_or_the_contract_off_the_image_routes_pass_every_address_as_before() {
        let contract_off = [
            (ENABLED_VAR, "0"),
            (HOSTED_IMAGE_HOSTS_VAR, "cdn.example.org"),
        ];
        for url in [
            "http://anywhere.test/a.png",
            "https://10.0.0.1/a.png",
            "https://user@x.test/a.png",
            " https://x.test/a.png",
            "ftp://x.test/a.png",
        ] {
            assert!(image_route_passes(&[], url), "{url:?}");
            assert!(
                image_route_passes(&contract_off, url),
                "{url:?}: the contract is off"
            );
        }
    }

    #[test]
    fn a_refused_image_address_is_typed_and_never_echoed() {
        let refusal = policy(&[(HOSTED_IMAGE_HOSTS_VAR, "cdn.example.org")])
            .check_hosted_image_url("https://evil.example.net/secret-chart.png")
            .expect_err("refused");
        assert_eq!(refusal.code, AttachmentCode::EncodingInvalid);
        assert_eq!(refusal.message, "The image address is not allowed.");
        assert!(
            !refusal.message.contains("evil.example.net"),
            "{}",
            refusal.message
        );
        assert!(!refusal.message.contains("secret"), "{}", refusal.message);
    }

    #[test]
    fn a_backslash_blank_or_non_ascii_byte_is_refused_even_when_the_host_is_listed() {
        // The URL standard reads the backslash as a path separator, so this names the listed host to
        // this check and an internal host to a parser that keeps the backslash in the authority.
        for url in [
            "https://cdn.example.org\\@10.0.0.1/a.png",
            "https://cdn.example.org/a\\b.png",
            "https://cdn.example.org/a b.png",
            "https://cdn.example.org/a\tb.png",
            "https://cdn.example.org/a\u{7f}.png",
            "https://cdn.example.org/\u{e5}.png",
        ] {
            assert!(!hosted_passes("cdn.example.org", url), "{url:?}");
        }
        assert!(hosted_passes(
            "cdn.example.org",
            "https://cdn.example.org/a.png"
        ));
    }
}
