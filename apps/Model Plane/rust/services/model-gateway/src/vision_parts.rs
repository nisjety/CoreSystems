//! Images as parts of the turn (WS08-S4).
//!
//! Until now any image on a chat turn diverted the whole turn to `AnalyzeImage`
//! (`vision::select_image` then `vision_stream`): one image, no history, no tools,
//! no memory. With `CHAT_VISION_AS_PARTS_ENABLED` on, an Ask turn whose picked
//! model reads images goes down the ordinary path instead, and the decoded images
//! ride on every `InferRequest` of the turn (`InferRequest.image_parts`, tag 20)
//! beside the message that carries them: every tool-decision round and the final
//! answer see the same pictures.
//!
//! The images are request-scope memory only. This module never stores, logs or
//! serializes the bytes (counts and sizes only), and a zero-retention turn never
//! reaches it: `zdr_features` refuses attachments first, and [`admit`] refuses
//! again rather than rely on that order.
//!
//! The fallback to the diversion is deliberate and narrow: only while the switch
//! is off, or when no model that reads images can serve the turn. A Do turn with
//! an image is refused (`attachment_do_unsupported`) until the Do loop carries
//! attachments (WS08-S6), because the diversion would silently drop its tools.

use base64::Engine as _;
use mp_contracts::model_plane::v1::{ChatMessage, InferImagePart, InferRequest, ModelInfo};

use crate::attachment_policy::{AttachmentCode, IMAGE_MIMES};
use super::{is_image, InvokeAttachment};

/// Opt-in switch, default off: only the literal `true` enables it.
pub const ENABLED_VAR: &str = "CHAT_VISION_AS_PARTS_ENABLED";

/// A Do turn that carries an image. Not one of the five closed policy codes: it is
/// a property of the loop, lifted when the Do loop can carry the bytes (S6).
pub const DO_UNSUPPORTED_CODE: &str = "attachment_do_unsupported";

/// The feature flag inference-core lists on a model that reads images.
const VISION_FEATURE: &str = "vision";

/// Image parts one request may carry. inference-core refuses a request with more (its
/// `MAX_IMAGE_PARTS`), as `invalid_argument` and only after the run exists, so the gateway keeps
/// the same count and refuses the turn before any run.
const MAX_IMAGE_PARTS: usize = 4;

#[must_use]
pub fn enabled() -> bool {
    enabled_with(&|key| std::env::var(key).ok())
}

fn enabled_with(get: &dyn Fn(&str) -> Option<String>) -> bool {
    get(ENABLED_VAR).is_some_and(|value| value.trim() == "true")
}

/// A typed refusal: a stable code for the client and a sentence for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartsRefusal {
    pub code: &'static str,
    pub message: String,
}

impl PartsRefusal {
    fn new(code: &'static str, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
        }
    }
}

struct Image {
    data: Vec<u8>,
    mime_type: String,
}

/// The decoded images of one turn and the message they belong to. Empty for every
/// turn without images, with the switch off and when the diversion serves the turn.
#[derive(Default)]
pub struct ImageParts {
    images: Vec<Image>,
    /// The user message the images are part of, as the model sees the prompt.
    user_index: usize,
    /// That message's text, so the images find it again after history is shed.
    carrier: Option<ChatMessage>,
}

impl std::fmt::Debug for ImageParts {
    // Counts only: this type holds customer bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageParts")
            .field("images", &self.images.len())
            .field("user_index", &self.user_index)
            .finish()
    }
}

impl ImageParts {
    /// A turn with no images to carry.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Count and total size, for a log line that carries no content.
    #[must_use]
    pub fn summary(&self) -> (usize, usize) {
        (
            self.images.len(),
            self.images.iter().map(|image| image.data.len()).sum(),
        )
    }

    /// Decodes the image attachments of a turn the attachment policy has already
    /// judged. Other kinds are not images and are left to their own path. Nothing
    /// is dropped: an image that cannot travel as bytes (a hosted URL is never
    /// fetched for the caller), cannot be decoded, or is not one of the allowed
    /// types refuses the turn.
    ///
    /// # Errors
    /// The typed refusal for the first image that cannot be carried.
    pub fn from_validated(attachments: &[InvokeAttachment]) -> Result<Self, PartsRefusal> {
        let mut images = Vec::new();
        for attachment in attachments.iter().filter(|attachment| is_image(attachment)) {
            let mime_type = attachment
                .mime_type
                .as_deref()
                .unwrap_or_default()
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            if !IMAGE_MIMES.contains(&mime_type.as_str()) {
                return Err(PartsRefusal::new(
                    AttachmentCode::MediaUnsupported.as_str(),
                    "Images must be PNG, JPEG, WebP or GIF.",
                ));
            }
            // The same tolerance the policy and the diversion have: a `data:` prefix.
            let payload = attachment
                .data_base64
                .as_deref()
                .filter(|data| !data.is_empty())
                .map(|data| data.rsplit(',').next().unwrap_or(data))
                .ok_or_else(|| {
                    PartsRefusal::new(
                        AttachmentCode::EncodingInvalid.as_str(),
                        "An image must be sent as bytes; a link to an image is not fetched.",
                    )
                })?;
            let data = base64::engine::general_purpose::STANDARD
                .decode(payload)
                .ok()
                .filter(|data| !data.is_empty())
                .ok_or_else(|| {
                    PartsRefusal::new(
                        AttachmentCode::EncodingInvalid.as_str(),
                        "An attached image could not be read as sent.",
                    )
                })?;
            images.push(Image { data, mime_type });
        }
        Ok(Self {
            images,
            ..Self::default()
        })
    }

    /// Adds the gateway-verified browser region screenshots of the turn (QF-10,
    /// `browser_page_context`) after any attached images, in region order. They are
    /// request-scope memory like every other part: never stored, logged or serialized.
    /// Must run before [`ImageParts::bind`].
    ///
    /// # Errors
    /// `attachment_count_exceeded` when the attached images and the regions together
    /// exceed [`MAX_IMAGE_PARTS`]. Nothing is added then, so the caller refuses the turn.
    pub fn push_browser_regions<'a>(
        &mut self,
        images: impl IntoIterator<Item = (&'a [u8], &'a str)>,
    ) -> Result<(), PartsRefusal> {
        let regions: Vec<(&[u8], &str)> = images.into_iter().collect();
        let total = self.images.len() + regions.len();
        if total > MAX_IMAGE_PARTS {
            return Err(PartsRefusal {
                code: AttachmentCode::CountExceeded.as_str(),
                message: format!(
                    "At most {MAX_IMAGE_PARTS} images can be attached to one message, browser regions included ({total} were sent)."
                ),
            });
        }
        for (data, mime_type) in regions {
            self.images.push(Image {
                data: data.to_vec(),
                mime_type: mime_type.to_owned(),
            });
        }
        Ok(())
    }

    /// Binds the images to the current turn's user message in the rendered prompt:
    /// the last `user` message, since the prompt ends with the turn being answered
    /// (system blocks and prior history come before it). A no-op without images.
    #[must_use]
    pub fn bind(mut self, messages: &[ChatMessage]) -> Self {
        if self.images.is_empty() {
            return self;
        }
        if let Some(index) = messages.iter().rposition(|message| message.role == "user") {
            self.user_index = index;
            self.carrier = Some(messages[index].clone());
        }
        self
    }
}

/// Decides how a turn's images travel. `Ok(none)` keeps today's behaviour (the
/// diversion decides); `Ok(parts)` means the turn takes the ordinary path with the
/// images attached.
///
/// `reads_images` is whether a model that reads images can serve this turn; the
/// caller asks inference-core only when the switch is on and the turn has images.
///
/// # Errors
/// A Do turn with an image, a ZDR turn with one, or an image that cannot be carried.
pub fn admit(
    enabled: bool,
    agentic: bool,
    zdr: bool,
    attachments: &[InvokeAttachment],
    reads_images: bool,
) -> Result<ImageParts, PartsRefusal> {
    if !enabled || !has_images(attachments) {
        return Ok(ImageParts::none());
    }
    if zdr {
        return Err(PartsRefusal::new(
            AttachmentCode::MediaUnsupported.as_str(),
            "Images are not read on a zero-retention turn.",
        ));
    }
    if agentic {
        return Err(PartsRefusal::new(
            DO_UNSUPPORTED_CODE,
            "Images cannot be attached to a Do turn yet. Send the message in Ask mode.",
        ));
    }
    if !reads_images {
        return Ok(ImageParts::none());
    }
    ImageParts::from_validated(attachments)
}

/// Whether any attachment is an image.
#[must_use]
pub fn has_images(attachments: &[InvokeAttachment]) -> bool {
    attachments.iter().any(is_image)
}

/// Whether the picked model reads images, from inference-core's catalogue. A model
/// the catalogue lists answers for itself; anything else is a Verevon mode (not a
/// catalogue entry), which can be served whenever some listed model reads images.
#[must_use]
pub fn route_reads_images(models: &[ModelInfo], picked: &str) -> bool {
    let reads = |model: &ModelInfo| model.features.iter().any(|feature| feature == VISION_FEATURE);
    match models.iter().find(|model| model.id == picked) {
        Some(model) => reads(model),
        None => models.iter().any(reads),
    }
}

/// Puts the images on `request` at the user message they belong to. The request
/// keeps its own list untouched when the turn has no images. The prompt only ever
/// grows after the images are bound, so the index still names the same message.
pub fn attach(request: &mut InferRequest, parts: &ImageParts) {
    if parts.is_empty() {
        return;
    }
    debug_assert!(
        carrier_at(&request.messages, parts, parts.user_index),
        "the prompt moved under the images: only appends are allowed after they are bound"
    );
    let message_index = u32::try_from(parts.user_index).unwrap_or(u32::MAX);
    request.image_parts = parts
        .images
        .iter()
        .map(|image| InferImagePart {
            message_index,
            data: image.data.clone(),
            mime_type: image.mime_type.clone(),
        })
        .collect();
}

/// Re-points the images of `request` after its history was shed to fit the
/// provider's input limit. Returns false when the message that carried them is
/// gone, so the caller fails the turn instead of sending images with no message.
pub fn rebase(request: &mut InferRequest, parts: &ImageParts) -> bool {
    if parts.is_empty() {
        return true;
    }
    let Some(carrier) = &parts.carrier else {
        return false;
    };
    let Some(index) = request
        .messages
        .iter()
        .rposition(|message| message.role == carrier.role && message.content == carrier.content)
    else {
        return false;
    };
    let message_index = u32::try_from(index).unwrap_or(u32::MAX);
    for part in &mut request.image_parts {
        part.message_index = message_index;
    }
    true
}

fn carrier_at(messages: &[ChatMessage], parts: &ImageParts, index: usize) -> bool {
    match (&parts.carrier, messages.get(index)) {
        (Some(carrier), Some(message)) => {
            message.role == carrier.role && message.content == carrier.content
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // "hi" and "ho" as base64.
    const HI: &str = "aGk=";
    const HO: &str = "aG8=";

    fn image(data: &str, mime: &str) -> InvokeAttachment {
        InvokeAttachment {
            kind: Some("image".to_owned()),
            data_base64: Some(data.to_owned()),
            mime_type: Some(mime.to_owned()),
            ..Default::default()
        }
    }

    fn message(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        }
    }

    fn prompt() -> Vec<ChatMessage> {
        vec![
            message("system", "rules"),
            message("user", "earlier"),
            message("assistant", "answer"),
            message("system", "memory"),
            message("user", "what is in these?"),
        ]
    }

    fn model(id: &str, features: &[&str]) -> ModelInfo {
        ModelInfo {
            id: id.to_owned(),
            features: features.iter().map(|f| (*f).to_owned()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn the_switch_is_opt_in_and_reads_only_the_literal_true() {
        let with = |value: Option<&str>| {
            enabled_with(&|key| {
                (key == ENABLED_VAR)
                    .then(|| value.map(str::to_owned))
                    .flatten()
            })
        };
        assert!(!with(None));
        assert!(!with(Some("1")));
        assert!(!with(Some("TRUE")));
        assert!(!with(Some("yes")));
        assert!(with(Some("true")));
        assert!(with(Some(" true ")));
    }

    #[test]
    fn two_images_decode_and_bind_to_the_last_user_message() {
        let parts = ImageParts::from_validated(&[
            image(HI, "image/png"),
            image(&format!("data:image/jpeg;base64,{HO}"), "image/JPEG"),
        ])
        .expect("both images decode")
        .bind(&prompt());
        assert_eq!(parts.summary(), (2, 4));
        let mut request = InferRequest {
            messages: prompt(),
            ..Default::default()
        };
        attach(&mut request, &parts);
        assert_eq!(request.image_parts.len(), 2);
        assert!(request.image_parts.iter().all(|part| part.message_index == 4));
        assert_eq!(request.image_parts[0].data, b"hi");
        assert_eq!(request.image_parts[1].data, b"ho");
        assert_eq!(request.image_parts[1].mime_type, "image/jpeg");
    }

    #[test]
    fn no_images_leaves_the_request_untouched() {
        let mut request = InferRequest {
            messages: prompt(),
            ..Default::default()
        };
        let before = format!("{request:?}");
        attach(&mut request, &ImageParts::none().bind(&prompt()));
        assert_eq!(format!("{request:?}"), before);
        // A file is not an image: it takes the sandbox path, not this one.
        let file = InvokeAttachment {
            kind: Some("file".to_owned()),
            name: Some("a.csv".to_owned()),
            data_base64: Some(HI.to_owned()),
            mime_type: Some("text/csv".to_owned()),
            ..Default::default()
        };
        assert!(ImageParts::from_validated(&[file]).expect("no images").is_empty());
    }

    #[test]
    fn a_hosted_link_bad_base64_and_a_foreign_type_each_refuse_the_turn() {
        let link = InvokeAttachment {
            kind: Some("image".to_owned()),
            url: Some("https://x/y.png".to_owned()),
            mime_type: Some("image/png".to_owned()),
            ..Default::default()
        };
        let refusal = ImageParts::from_validated(&[link]).expect_err("a link is not fetched");
        assert_eq!(refusal.code, "attachment_encoding_invalid");
        let refusal = ImageParts::from_validated(&[image("!!!", "image/png")])
            .expect_err("bad base64 is not skipped");
        assert_eq!(refusal.code, "attachment_encoding_invalid");
        let refusal = ImageParts::from_validated(&[image(HI, "image/svg+xml")])
            .expect_err("svg is not an allowed type");
        assert_eq!(refusal.code, "attachment_media_unsupported");
    }

    #[test]
    fn admit_keeps_the_diversion_while_the_switch_is_off_or_no_model_reads_images() {
        let attachments = [image(HI, "image/png")];
        assert!(admit(false, false, false, &attachments, true).unwrap().is_empty());
        assert!(admit(false, true, false, &attachments, true).unwrap().is_empty());
        assert!(admit(true, false, false, &attachments, false).unwrap().is_empty());
        assert!(admit(true, false, false, &[], true).unwrap().is_empty());
        assert!(!admit(true, false, false, &attachments, true).unwrap().is_empty());
    }

    #[test]
    fn a_do_turn_and_a_zdr_turn_with_an_image_are_refused_typed() {
        let attachments = [image(HI, "image/png")];
        let refusal = admit(true, true, false, &attachments, true).unwrap_err();
        assert_eq!(refusal.code, DO_UNSUPPORTED_CODE);
        let refusal = admit(true, false, true, &attachments, true).unwrap_err();
        assert_eq!(refusal.code, "attachment_media_unsupported");
    }

    #[test]
    fn a_listed_model_answers_for_itself_and_a_mode_for_the_catalogue() {
        let models = [
            model("gpt-4o", &["tools", "vision"]),
            model("deepseek-chat", &["tools"]),
        ];
        assert!(route_reads_images(&models, "gpt-4o"));
        assert!(!route_reads_images(&models, "deepseek-chat"));
        assert!(route_reads_images(&models, "verevon-balance"));
        assert!(!route_reads_images(&models[1..], "verevon-balance"));
        assert!(!route_reads_images(&[], "verevon-balance"));
    }

    #[test]
    fn appending_after_the_bind_does_not_move_the_images() {
        let parts = ImageParts::from_validated(&[image(HI, "image/png")])
            .unwrap()
            .bind(&prompt());
        let mut grown = prompt();
        grown.push(message("user", "tool result context"));
        grown.push(message("system", "a note"));
        let mut request = InferRequest {
            messages: grown,
            ..Default::default()
        };
        attach(&mut request, &parts);
        assert_eq!(request.image_parts[0].message_index, 4);
    }

    #[test]
    fn rebase_follows_the_carrier_after_history_is_shed() {
        let parts = ImageParts::from_validated(&[image(HI, "image/png")])
            .unwrap()
            .bind(&prompt());
        let mut request = InferRequest {
            messages: prompt(),
            ..Default::default()
        };
        attach(&mut request, &parts);
        // Two history messages go and a notice takes a slot at the front.
        request.messages.drain(1..3);
        request.messages.insert(0, message("system", "dropped notice"));
        assert!(rebase(&mut request, &parts));
        assert_eq!(request.image_parts[0].message_index, 3);
        assert_eq!(request.messages[3].content, "what is in these?");
        // The carrier itself shed: the caller must fail the turn.
        request.messages.retain(|message| message.content != "what is in these?");
        assert!(!rebase(&mut request, &parts));
    }

    #[test]
    fn debug_never_prints_the_bytes() {
        let parts = ImageParts::from_validated(&[image(HI, "image/png")])
            .unwrap()
            .bind(&prompt());
        let shown = format!("{parts:?}");
        assert!(shown.contains("images: 1"), "{shown}");
        assert!(!shown.contains("104"), "{shown}");
    }

    #[test]
    fn browser_regions_share_the_image_part_cap_with_the_attached_images() {
        let shot: (&[u8], &str) = (b"hi", "image/png");
        let two_images = [image(HI, "image/png"), image(HO, "image/png")];
        // Two attached images and two regions make four, the most inference-core takes.
        let mut fits = ImageParts::from_validated(&two_images).expect("both images decode");
        fits.push_browser_regions([shot, shot])
            .expect("four image parts fit");
        assert_eq!(fits.summary().0, 4);
        // Two attached images and three regions make five: refused typed, and nothing is added.
        let mut over = ImageParts::from_validated(&two_images).expect("both images decode");
        let refusal = over
            .push_browser_regions([shot, shot, shot])
            .expect_err("five image parts are refused");
        assert_eq!(refusal.code, "attachment_count_exceeded");
        assert!(
            refusal.message.contains("(5 were sent)"),
            "{}",
            refusal.message
        );
        assert_eq!(over.summary().0, 2);
        // Regions alone may fill the cap: the browser allows at most four of them.
        let mut alone = ImageParts::none();
        alone
            .push_browser_regions([shot, shot, shot, shot])
            .expect("four regions fit");
        assert_eq!(alone.summary().0, 4);
    }
}
