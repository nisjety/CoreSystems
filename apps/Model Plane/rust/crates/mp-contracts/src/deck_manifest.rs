//! The slide manifest of a deck: the source of truth the gateway's `deck` tool owns (EP-5 S2).
//!
//! # Why the manifest and not the file
//!
//! A sandbox workspace is thrown away after each call, so the `.pptx` cannot be
//! the deck. It is rebuilt from this manifest by the fixed builder
//! (`skills/office/pptx/scripts/build_deck.py`) whenever the model asks for a
//! build. The model never writes the manifest: it sends typed operations
//! ([`DeckOp`]), and [`DeckManifest::apply`] checks each one against the caps
//! before any sandbox call, mints the slide ids, and refuses a slide that breaks
//! a rule with the path of the field at fault.
//!
//! # Who sets a slide's state
//!
//! Only the builder's report ([`DeckManifest::apply_report`]). An operation that
//! changes a slide puts it back to [`SlideState::Draft`], and an operation has no
//! field to claim a state with. The report is read from sandbox output, so
//! [`BuildReport::from_untrusted_value`] caps and scrubs it before it is used.
//!
//! The caps are EP-5 decision D11 and must equal the builder's; a test reads the
//! script to keep the two in step.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The manifest format this contract and the builder read.
pub const MANIFEST_VERSION: u32 = 1;

/// Longest manifest, in characters of its canonical JSON.
pub const MAX_MANIFEST_CHARS: usize = 120_000;

/// Most slides in one deck.
pub const MAX_SLIDES: usize = 40;

/// Most list items on one slide: bullets, left and right together.
pub const MAX_BULLETS: usize = 6;

/// Longest list item, in characters.
pub const MAX_BULLET_CHARS: usize = 90;

/// Longest deck or slide title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;

/// Longest speaker notes of one slide, in characters.
pub const MAX_NOTES_CHARS: usize = 2_000;

/// Longest deck or slide id, in characters. The builder refuses a longer slide id.
pub const MAX_ID_CHARS: usize = 64;

/// Longest picture path, in characters.
pub const MAX_IMAGE_PATH_CHARS: usize = 255;

/// Most findings kept per slide from a builder report.
pub const MAX_FINDINGS: usize = 8;

/// Longest finding message kept, in characters.
pub const MAX_FINDING_MESSAGE_CHARS: usize = 200;

/// Most sources a deck carries from the document it was made from (EP-5 S8).
pub const MAX_SOURCES: usize = 40;

/// Highest `[n]` a source may be cited as.
pub const MAX_CITATION_NUMBER: u32 = 999;

/// Longest source title kept, in characters.
pub const MAX_SOURCE_TITLE_CHARS: usize = 160;

/// Longest source link or document id kept, in characters.
pub const MAX_SOURCE_URL_CHARS: usize = 400;

/// Most unverified research claims a deck carries.
pub const MAX_UNSUPPORTED_CLAIMS: usize = 32;

/// Longest unverified claim kept, in characters.
pub const MAX_CLAIM_CHARS: usize = 300;

/// Longest artifact id a deck's source names, in characters.
pub const MAX_SOURCE_ARTIFACT_ID_CHARS: usize = 128;

/// How one slide is laid out. Each maps to one layout of the builder's default
/// template, which places only some of the content fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlideLayout {
    /// A title slide; its bullets become the subtitle.
    Title,
    TitleBullets,
    TitleOnly,
    Section,
    /// Two lists side by side, from `left` and `right`.
    TwoColumn,
    /// A title and a picture from `image`.
    Image,
}

impl SlideLayout {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::TitleBullets => "title_bullets",
            Self::TitleOnly => "title_only",
            Self::Section => "section",
            Self::TwoColumn => "two_column",
            Self::Image => "image",
        }
    }

    /// Whether the builder places `bullets`, `left`, `right` or `image` on this
    /// layout. A field it does not place would be left out of the file, so the
    /// manifest refuses it instead of keeping text the user never sees.
    fn places(self, field: &str) -> bool {
        match self {
            Self::Title | Self::TitleBullets | Self::Section => field == "bullets",
            Self::TitleOnly => false,
            Self::TwoColumn => matches!(field, "left" | "right"),
            Self::Image => field == "image",
        }
    }
}

/// Where a slide stands. Only a builder report moves a slide out of `Draft`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlideState {
    /// Changed since the last build, or never built.
    Draft,
    Ok,
    /// Built, with findings the model should look at.
    Warn,
    /// Not built as asked: the slide must be fixed.
    Error,
}

impl SlideState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// One thing the builder found about one slide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideFinding {
    pub id: String,
    pub message: String,
}

/// One slide as the manifest keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slide {
    /// Minted by the manifest (`s1`, `s2`, ...), never by the model.
    pub id: String,
    pub layout: SlideLayout,
    pub title: String,
    #[serde(default)]
    pub bullets: Vec<String>,
    #[serde(default)]
    pub left: Vec<String>,
    #[serde(default)]
    pub right: Vec<String>,
    /// A path relative to the sandbox working directory.
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub state: SlideState,
    #[serde(default)]
    pub findings: Vec<SlideFinding>,
}

/// A slide's content as an operation supplies it: no id, no state, no findings,
/// so the model can neither pick an id nor claim a slide is fine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideInput {
    pub layout: SlideLayout,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub bullets: Vec<String>,
    #[serde(default)]
    pub left: Vec<String>,
    #[serde(default)]
    pub right: Vec<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// One change to a deck. `after_id` absent means the first position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DeckOp {
    Create {
        title: String,
        locale: String,
        #[serde(default)]
        slides: Vec<SlideInput>,
    },
    SetSlide {
        id: String,
        slide: SlideInput,
    },
    InsertSlide {
        #[serde(default)]
        after_id: Option<String>,
        slide: SlideInput,
    },
    DeleteSlide {
        id: String,
    },
    MoveSlide {
        id: String,
        #[serde(default)]
        after_id: Option<String>,
    },
    /// Build the deck as it stands; changes no content.
    Build,
}

impl DeckOp {
    /// Whether the operation changes the deck's content.
    #[must_use]
    pub const fn changes_content(&self) -> bool {
        !matches!(self, Self::Build)
    }
}

/// Why an operation or a stored manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckOpError {
    /// A field breaks a rule. `path` names it, for example `slides[2].bullets[0]`.
    Invalid { path: String, message: String },
    /// No slide in the deck has this id.
    UnknownSlide(String),
    /// Only `create` may start a deck.
    NotCreated,
    /// `create` on a deck that already exists.
    AlreadyCreated,
    /// A build of a deck with no slides.
    EmptyDeck,
}

impl fmt::Display for DeckOpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { path, message } => write!(f, "{path}: {message}"),
            Self::UnknownSlide(id) => write!(
                f,
                "no slide has the id '{}' in this deck",
                truncate_chars(id, MAX_ID_CHARS)
            ),
            Self::NotCreated => f.write_str("the deck does not exist yet; start it with the create operation"),
            Self::AlreadyCreated => f.write_str(
                "the deck already exists; change it with set_slide, insert_slide, delete_slide or move_slide",
            ),
            Self::EmptyDeck => f.write_str("the deck has no slides to build"),
        }
    }
}

impl std::error::Error for DeckOpError {}

fn invalid(path: impl Into<String>, message: impl Into<String>) -> DeckOpError {
    DeckOpError::Invalid {
        path: path.into(),
        message: message.into(),
    }
}

/// The deck: its slides in order, and the counters that keep ids unique.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckManifest {
    pub version: u32,
    /// The artifact id the deck is kept under in the thread. Minted by the gateway.
    pub deck_id: String,
    pub title: String,
    /// A language tag such as `nb-NO`.
    pub locale: String,
    pub slides: Vec<Slide>,
    /// Content operations applied so far; 0 is a deck not yet created.
    pub revision: u32,
    /// The number the next minted slide id takes. It only grows, so the id of a
    /// deleted slide is never given to another one.
    pub next_slide: u32,
    /// The brand template the deck was last built with (EP-5 S6), set by the gateway from the turn's resolved pick, never by an operation. Absent for a deck in the default theme, so its canonical text is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<crate::deck_theme::DeckTemplateRef>,
    /// The document the deck was made from (EP-5 S8). Absent writes no key, so a
    /// deck made without one keeps exactly the manifest text it had before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<DeckSource>,
}

/// One source the deck's document cites, by the `[n]` it cites it as. The
/// builder names it in the notes of each slide whose bullets cite it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckCitation {
    pub number: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
}

/// The document a deck was made from (EP-5 S8). The gateway sets it from the
/// thread's own artifact store when the deck is created; no operation carries
/// it, so the model cannot name a source the thread does not hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckSource {
    pub artifact_id: String,
    /// The document's version when the deck was made from it.
    pub version: u32,
    /// sha256 (hex) of the document's provenance stamp, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_digest: Option<String>,
    /// The stamp's `ledger_digest`, when the research kept a claim ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_digest: Option<String>,
    /// The stamp's read sources, by number.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<DeckCitation>,
    /// Claims the ledger marked unsupported. A bullet that states one without
    /// "(ikke verifisert)" is a builder `warn`, never silently kept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_claims: Vec<String>,
}

impl DeckSource {
    fn validate(&self) -> Result<(), DeckOpError> {
        let id = &self.artifact_id;
        if id.trim().is_empty() || id.chars().count() > MAX_SOURCE_ARTIFACT_ID_CHARS || id.chars().any(char::is_control) {
            return Err(invalid(
                "source.artifact_id",
                format!("a source artifact id is 1 to {MAX_SOURCE_ARTIFACT_ID_CHARS} printable characters"),
            ));
        }
        if self.version == 0 {
            return Err(invalid("source.version", "a source version starts at 1"));
        }
        for (field, digest) in [("provenance_digest", &self.provenance_digest), ("ledger_digest", &self.ledger_digest)] {
            if digest.as_deref().is_some_and(|digest| !is_sha256_hex(digest)) {
                return Err(invalid(format!("source.{field}"), "a digest is 64 lowercase hex characters"));
            }
        }
        if self.sources.len() > MAX_SOURCES {
            return Err(invalid("source.sources", format!("at most {MAX_SOURCES} sources")));
        }
        for (index, citation) in self.sources.iter().enumerate() {
            let path = format!("source.sources[{index}]");
            if citation.number == 0 || citation.number > MAX_CITATION_NUMBER {
                return Err(invalid(format!("{path}.number"), format!("a source number is 1 to {MAX_CITATION_NUMBER}")));
            }
            if self.sources[..index].iter().any(|earlier| earlier.number == citation.number) {
                return Err(invalid(format!("{path}.number"), "each source number is listed once"));
            }
            if !is_one_line(&citation.title, MAX_SOURCE_TITLE_CHARS) {
                return Err(invalid(format!("{path}.title"), format!("one line of at most {MAX_SOURCE_TITLE_CHARS} characters")));
            }
            for (field, value) in [("url", &citation.url), ("document_id", &citation.document_id)] {
                if value.as_deref().is_some_and(|value| value.is_empty() || !is_one_line(value, MAX_SOURCE_URL_CHARS)) {
                    return Err(invalid(format!("{path}.{field}"), format!("one line of 1 to {MAX_SOURCE_URL_CHARS} characters")));
                }
            }
        }
        if self.unsupported_claims.len() > MAX_UNSUPPORTED_CLAIMS {
            return Err(invalid("source.unsupported_claims", format!("at most {MAX_UNSUPPORTED_CLAIMS} claims")));
        }
        for (index, claim) in self.unsupported_claims.iter().enumerate() {
            if claim.trim().is_empty() || !is_one_line(claim, MAX_CLAIM_CHARS) {
                return Err(invalid(
                    format!("source.unsupported_claims[{index}]"),
                    format!("one line of 1 to {MAX_CLAIM_CHARS} characters"),
                ));
            }
        }
        Ok(())
    }

    /// The keys the builder reads from the source: the sources it writes into
    /// notes and the claims it warns on. The digests stay out; they are the
    /// thread's record, not layout input.
    fn add_to_builder_view(&self, view: &mut Value) {
        if !self.sources.is_empty() {
            view["sources"] = json!(self.sources);
        }
        if !self.unsupported_claims.is_empty() {
            view["unsupported_claims"] = json!(self.unsupported_claims);
        }
    }
}

impl DeckManifest {
    /// A deck under `deck_id` that only a `create` operation can start.
    #[must_use]
    pub fn empty(deck_id: &str) -> Self {
        Self {
            version: MANIFEST_VERSION,
            deck_id: deck_id.to_owned(),
            title: String::new(),
            locale: String::new(),
            slides: Vec::new(),
            revision: 0,
            next_slide: 1,
            template: None,
            source: None,
        }
    }

    /// Apply one operation. All or nothing: a refused operation leaves the
    /// manifest as it was.
    ///
    /// # Errors
    /// When the operation names a slide the deck does not have, comes in the
    /// wrong order (`create` twice, anything before `create`), or leaves a field
    /// that breaks a cap or a layout rule; then the error names that field.
    pub fn apply(&mut self, op: DeckOp) -> Result<(), DeckOpError> {
        let mut next = self.clone();
        next.apply_unchecked(op)?;
        next.validate()?;
        *self = next;
        Ok(())
    }

    fn apply_unchecked(&mut self, op: DeckOp) -> Result<(), DeckOpError> {
        match op {
            DeckOp::Create { .. } if self.revision > 0 => return Err(DeckOpError::AlreadyCreated),
            DeckOp::Create {
                title,
                locale,
                slides,
            } => {
                self.title = title.trim().to_owned();
                self.locale = locale.trim().to_owned();
                for input in slides {
                    let slide = self.mint(input);
                    self.slides.push(slide);
                }
            }
            _ if self.revision == 0 => return Err(DeckOpError::NotCreated),
            DeckOp::SetSlide { id, slide } => {
                let index = self.index_of(&id)?;
                let kept = self.slides[index].id.clone();
                self.slides[index] = Slide::draft(kept, slide);
            }
            DeckOp::InsertSlide { after_id, slide } => {
                let at = self.position_after(after_id.as_deref())?;
                let slide = self.mint(slide);
                self.slides.insert(at, slide);
            }
            DeckOp::DeleteSlide { id } => {
                let index = self.index_of(&id)?;
                self.slides.remove(index);
            }
            DeckOp::MoveSlide { id, after_id } => {
                let index = self.index_of(&id)?;
                if after_id.as_deref() == Some(id.as_str()) {
                    return Err(invalid("after_id", "a slide cannot move after itself"));
                }
                let slide = self.slides.remove(index);
                let at = self.position_after(after_id.as_deref())?;
                self.slides.insert(at, slide);
            }
            DeckOp::Build => return self.ensure_buildable(),
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    fn mint(&mut self, input: SlideInput) -> Slide {
        let id = format!("s{}", self.next_slide);
        self.next_slide = self.next_slide.saturating_add(1);
        Slide::draft(id, input)
    }

    fn index_of(&self, id: &str) -> Result<usize, DeckOpError> {
        let id = id.trim();
        self.slides
            .iter()
            .position(|slide| slide.id == id)
            .ok_or_else(|| DeckOpError::UnknownSlide(id.to_owned()))
    }

    fn position_after(&self, anchor: Option<&str>) -> Result<usize, DeckOpError> {
        anchor.map_or(Ok(0), |anchor| self.index_of(anchor).map(|index| index + 1))
    }

    /// Whether the deck can be handed to the builder.
    ///
    /// # Errors
    /// When the deck was never created or has no slides.
    pub fn ensure_buildable(&self) -> Result<(), DeckOpError> {
        if self.revision == 0 {
            Err(DeckOpError::NotCreated)
        } else if self.slides.is_empty() {
            Err(DeckOpError::EmptyDeck)
        } else {
            Ok(())
        }
    }

    /// Check every rule the manifest holds to.
    ///
    /// # Errors
    /// The first field that breaks a rule, with its path.
    pub fn validate(&self) -> Result<(), DeckOpError> {
        if self.version != MANIFEST_VERSION {
            return Err(invalid("version", format!("the manifest version must be {MANIFEST_VERSION}")));
        }
        if !is_plain_id(&self.deck_id) {
            return Err(invalid(
                "deck_id",
                format!("a deck id is 1 to {MAX_ID_CHARS} letters, digits, '-' or '_'"),
            ));
        }
        if self.revision > 0 {
            check_title("title", &self.title)?;
            if !is_language_tag(&self.locale) {
                return Err(invalid("locale", "locale must be a language tag such as nb-NO"));
            }
        }
        if self.slides.len() > MAX_SLIDES {
            return Err(invalid(
                "slides",
                format!("a deck holds at most {MAX_SLIDES} slides; this one would have {}", self.slides.len()),
            ));
        }
        for (index, slide) in self.slides.iter().enumerate() {
            let path = format!("slides[{index}]");
            if self.slides[..index].iter().any(|earlier| earlier.id == slide.id)
                || !minted_below(&slide.id, self.next_slide)
            {
                return Err(invalid(format!("{path}.id"), "slide ids are minted by the deck and used once"));
            }
            slide.validate(&path)?;
        }
        if let Some(source) = &self.source {
            source.validate()?;
        }
        let chars = self.canonical_json().chars().count();
        if chars > MAX_MANIFEST_CHARS {
            return Err(invalid(
                "manifest",
                format!("the deck would be {chars} characters; the limit is {MAX_MANIFEST_CHARS}"),
            ));
        }
        Ok(())
    }

    /// Parse a stored manifest. It is read back from the thread, so it is held
    /// to every rule an operation is.
    ///
    /// # Errors
    /// When it is over the size cap, does not parse (an unknown field or state
    /// included), or breaks a rule.
    pub fn from_untrusted_json(raw: &str) -> Result<Self, DeckOpError> {
        if raw.chars().count() > MAX_MANIFEST_CHARS {
            return Err(invalid("manifest", format!("a stored deck is over {MAX_MANIFEST_CHARS} characters")));
        }
        let parsed: Self = serde_json::from_str(raw)
            .map_err(|error| invalid("manifest", truncate_chars(&error.to_string(), MAX_FINDING_MESSAGE_CHARS)))?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// The manifest as JSON with a fixed key order (the field order above), so
    /// the same deck always gives the same text and the same digest.
    #[must_use]
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The manifest v1 document the builder reads: content only. The state,
    /// findings and counters stay out, because the builder reports an unknown
    /// slide field as a finding. A deck made from a document adds its sources.
    #[must_use]
    pub fn builder_json(&self) -> String {
        let slides: Vec<Value> = self.slides.iter().map(Slide::builder_value).collect();
        let mut view = json!({
            "version": self.version,
            "title": self.title,
            "locale": self.locale,
            "slides": slides,
        });
        if let Some(source) = &self.source {
            source.add_to_builder_view(&mut view);
        }
        view.to_string()
    }

    /// Write a build's outcome onto the slides. A refused build marks every
    /// slide `error` with the reason; a slide the report leaves out is `error`
    /// too, since nothing says it was built.
    pub fn apply_report(&mut self, report: &BuildReport) {
        for slide in &mut self.slides {
            let (state, findings) = match (&report.refused, report.slides.iter().find(|entry| entry.id == slide.id)) {
                (Some(reason), _) => (SlideState::Error, vec![reason.clone()]),
                (None, Some(entry)) => (entry.state, entry.findings.clone()),
                (None, None) => (
                    SlideState::Error,
                    vec![SlideFinding {
                        id: "not_reported".to_owned(),
                        message: "the builder did not report on this slide".to_owned(),
                    }],
                ),
            };
            slide.state = state;
            slide.findings = findings;
        }
    }
}

impl Slide {
    fn draft(id: String, input: SlideInput) -> Self {
        Self {
            id,
            layout: input.layout,
            title: input.title.trim().to_owned(),
            bullets: items(input.bullets),
            left: items(input.left),
            right: items(input.right),
            image: present(input.image),
            notes: present(input.notes),
            state: SlideState::Draft,
            findings: Vec::new(),
        }
    }

    fn validate(&self, path: &str) -> Result<(), DeckOpError> {
        check_title(&format!("{path}.title"), &self.title)?;
        let lists = [("bullets", &self.bullets), ("left", &self.left), ("right", &self.right)];
        let count: usize = lists.iter().map(|(_, list)| list.len()).sum();
        if count > MAX_BULLETS {
            return Err(invalid(
                format!("{path}.bullets"),
                format!("slide {} has {count} list items; the limit is {MAX_BULLETS}, so split it", self.id),
            ));
        }
        for (field, list) in lists {
            if !list.is_empty() && !self.layout.places(field) {
                return Err(unplaced(path, field, self.layout));
            }
            for (index, item) in list.iter().enumerate() {
                let chars = item.chars().count();
                if chars > MAX_BULLET_CHARS {
                    return Err(invalid(
                        format!("{path}.{field}[{index}]"),
                        format!("{chars} characters; the limit is {MAX_BULLET_CHARS}"),
                    ));
                }
            }
        }
        if let Some(image) = &self.image {
            if !self.layout.places("image") {
                return Err(unplaced(path, "image", self.layout));
            }
            if let Some(problem) = image_path_problem(image) {
                return Err(invalid(format!("{path}.image"), problem));
            }
        }
        if let Some(notes) = &self.notes {
            let chars = notes.chars().count();
            if chars > MAX_NOTES_CHARS {
                return Err(invalid(
                    format!("{path}.notes"),
                    format!("{chars} characters; the limit is {MAX_NOTES_CHARS}"),
                ));
            }
        }
        if self.findings.len() > MAX_FINDINGS {
            return Err(invalid(format!("{path}.findings"), format!("at most {MAX_FINDINGS} findings")));
        }
        Ok(())
    }

    fn builder_value(&self) -> Value {
        let mut slide = Map::new();
        slide.insert("id".to_owned(), json!(self.id));
        slide.insert("layout".to_owned(), json!(self.layout.as_str()));
        slide.insert("title".to_owned(), json!(self.title));
        for (field, list) in [("bullets", &self.bullets), ("left", &self.left), ("right", &self.right)] {
            if !list.is_empty() {
                slide.insert(field.to_owned(), json!(list));
            }
        }
        if let Some(image) = &self.image {
            slide.insert("image".to_owned(), json!(image));
        }
        if let Some(notes) = &self.notes {
            slide.insert("notes".to_owned(), json!(notes));
        }
        Value::Object(slide)
    }
}

fn unplaced(path: &str, field: &str, layout: SlideLayout) -> DeckOpError {
    invalid(
        format!("{path}.{field}"),
        format!("layout {} does not place {field}; pick a layout that does or leave it out", layout.as_str()),
    )
}

fn check_title(path: &str, title: &str) -> Result<(), DeckOpError> {
    let chars = title.chars().count();
    if title.trim().is_empty() {
        Err(invalid(path, "a title is required"))
    } else if chars > MAX_TITLE_CHARS {
        Err(invalid(path, format!("{chars} characters; the limit is {MAX_TITLE_CHARS}")))
    } else {
        Ok(())
    }
}

/// List items trimmed, with empty ones dropped: the builder drops them too.
fn items(list: Vec<String>) -> Vec<String> {
    list.into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

fn present(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn is_plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_ID_CHARS
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_sha256_hex(digest: &str) -> bool {
    digest.len() == 64 && digest.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// One line of printable text no longer than `max` characters.
fn is_one_line(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

/// Whether `id` is a minted slide id (`s` and a number) below the next one to mint.
fn minted_below(id: &str, next_slide: u32) -> bool {
    id.strip_prefix('s')
        .filter(|digits| !digits.starts_with('0'))
        .and_then(|digits| digits.parse::<u32>().ok())
        .is_some_and(|number| number < next_slide)
}

/// The builder's language-tag rule: two or three letters, then subtags of one
/// to eight letters or digits.
fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let primary = parts.next().unwrap_or_default();
    (2..=3).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|part| (1..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Why a picture path may not be used, or `None`. A path is relative to the
/// working directory and may not climb out of it.
fn image_path_problem(path: &str) -> Option<String> {
    if path.chars().count() > MAX_IMAGE_PATH_CHARS {
        return Some(format!("an image path is at most {MAX_IMAGE_PATH_CHARS} characters"));
    }
    if path.chars().any(char::is_control) {
        return Some("an image path may not hold control characters".to_owned());
    }
    let bytes = path.as_bytes();
    if path.starts_with(['/', '\\']) || (bytes.len() > 1 && bytes[1] == b':') {
        return Some("an image path must be relative to the working directory".to_owned());
    }
    if path.split(['/', '\\']).any(|part| part == "..") {
        return Some("an image path may not contain '..'".to_owned());
    }
    None
}

/// What the builder reported for one slide, after capping and scrubbing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideReport {
    pub id: String,
    pub state: SlideState,
    pub findings: Vec<SlideFinding>,
}

/// The builder's one-line report on a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    pub slides: Vec<SlideReport>,
    /// Set when the builder refused the whole deck and wrote no file.
    pub refused: Option<SlideFinding>,
}

impl BuildReport {
    /// A build that did not happen, for a reason the gateway knows (the sandbox
    /// call failed, or its output held no report).
    #[must_use]
    pub fn refused(id: &str, message: &str) -> Self {
        Self {
            slides: Vec::new(),
            refused: Some(finding(id, message)),
        }
    }

    /// Read a report printed in a sandbox. Every list is capped and every string
    /// scrubbed, a state outside `ok`/`warn`/`error` reads as `error`, and a
    /// slide is never less severe than its findings show: `ok` with findings is
    /// `warn`.
    #[must_use]
    pub fn from_untrusted_value(value: &Value) -> Option<Self> {
        if let Some(reason) = value.get("refused").filter(|reason| !reason.is_null()) {
            return Some(Self {
                slides: Vec::new(),
                refused: Some(finding_from(reason)),
            });
        }
        let slides = value
            .get("slides")?
            .as_array()?
            .iter()
            .take(MAX_SLIDES)
            .filter_map(|entry| {
                let id = entry.get("id").and_then(Value::as_str)?;
                let findings: Vec<SlideFinding> = entry
                    .get("findings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .take(MAX_FINDINGS)
                    .map(finding_from)
                    .collect();
                let reported = match entry.get("state").and_then(Value::as_str) {
                    Some("ok") => SlideState::Ok,
                    Some("warn") => SlideState::Warn,
                    _ => SlideState::Error,
                };
                let state = if reported == SlideState::Ok && !findings.is_empty() {
                    SlideState::Warn
                } else {
                    reported
                };
                Some(SlideReport {
                    id: truncate_chars(id, MAX_ID_CHARS),
                    state,
                    findings,
                })
            })
            .collect();
        Some(Self { slides, refused: None })
    }
}

fn finding_from(value: &Value) -> SlideFinding {
    finding(
        value.get("id").and_then(Value::as_str).unwrap_or_default(),
        value.get("message").and_then(Value::as_str).unwrap_or_default(),
    )
}

/// A finding with its id reduced to `[a-z0-9_]` and its message to one line of
/// printable text, both capped.
fn finding(id: &str, message: &str) -> SlideFinding {
    let id: String = id
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .take(MAX_ID_CHARS)
        .collect();
    let message: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    SlideFinding {
        id: if id.is_empty() { "finding".to_owned() } else { id },
        message: truncate_chars(&message, MAX_FINDING_MESSAGE_CHARS),
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(layout: SlideLayout, title: &str, bullets: &[&str]) -> SlideInput {
        SlideInput {
            layout,
            title: title.to_owned(),
            bullets: bullets.iter().map(|&item| item.to_owned()).collect(),
            left: Vec::new(),
            right: Vec::new(),
            image: None,
            notes: None,
        }
    }

    fn created(slides: Vec<SlideInput>) -> DeckManifest {
        let mut deck = DeckManifest::empty("deck-q3");
        deck.apply(DeckOp::Create {
            title: "Kvartalstall".to_owned(),
            locale: "nb-NO".to_owned(),
            slides,
        })
        .expect("a valid create applies");
        deck
    }

    fn ids(deck: &DeckManifest) -> Vec<&str> {
        deck.slides.iter().map(|slide| slide.id.as_str()).collect()
    }

    #[test]
    fn ops_apply_in_order_and_ids_never_repeat() {
        let mut deck = created(vec![
            slide(SlideLayout::Title, "Kvartalstall", &[]),
            slide(SlideLayout::TitleBullets, "Salg", &["Opp 4 %"]),
        ]);
        assert_eq!(ids(&deck), ["s1", "s2"]);
        deck.apply(DeckOp::InsertSlide {
            after_id: Some("s1".to_owned()),
            slide: slide(SlideLayout::Section, "Del 1", &[]),
        })
        .unwrap();
        assert_eq!(ids(&deck), ["s1", "s3", "s2"]);
        deck.apply(DeckOp::DeleteSlide { id: "s2".to_owned() }).unwrap();
        deck.apply(DeckOp::InsertSlide {
            after_id: None,
            slide: slide(SlideLayout::TitleOnly, "Agenda", &[]),
        })
        .unwrap();
        // The deleted s2 is not handed out again.
        assert_eq!(ids(&deck), ["s4", "s1", "s3"]);
        deck.apply(DeckOp::MoveSlide {
            id: "s4".to_owned(),
            after_id: Some("s3".to_owned()),
        })
        .unwrap();
        assert_eq!(ids(&deck), ["s1", "s3", "s4"]);
        deck.apply(DeckOp::SetSlide {
            id: "s3".to_owned(),
            slide: slide(SlideLayout::Section, "Del en", &[]),
        })
        .unwrap();
        assert_eq!(deck.slides[1].id, "s3");
        assert_eq!(deck.slides[1].title, "Del en");
        assert_eq!(deck.revision, 6);
        // A build changes no content and no revision.
        deck.apply(DeckOp::Build).unwrap();
        assert_eq!(deck.revision, 6);
        assert_eq!(
            deck.apply(DeckOp::Create {
                title: "Igjen".to_owned(),
                locale: "nb".to_owned(),
                slides: Vec::new(),
            }),
            Err(DeckOpError::AlreadyCreated)
        );
        assert_eq!(
            DeckManifest::empty("deck-x").apply(DeckOp::Build),
            Err(DeckOpError::NotCreated)
        );
    }

    #[test]
    fn over_cap_slide_is_refused_with_field_path() {
        let mut deck = created(vec![slide(SlideLayout::TitleBullets, "Salg", &["Opp"])]);
        let before = deck.clone();
        let long = "x".repeat(MAX_BULLET_CHARS + 1);
        let error = deck
            .apply(DeckOp::InsertSlide {
                after_id: Some("s1".to_owned()),
                slide: slide(SlideLayout::TitleBullets, "Kostnader", &["kort", &long]),
            })
            .unwrap_err();
        assert!(
            matches!(&error, DeckOpError::Invalid { path, .. } if path == "slides[1].bullets[1]"),
            "{error}"
        );
        // Refused whole: the deck is unchanged and the id was not spent.
        assert_eq!(deck, before);

        let seven = ["a", "b", "c", "d", "e", "f", "g"];
        let error = deck
            .apply(DeckOp::SetSlide {
                id: "s1".to_owned(),
                slide: slide(SlideLayout::TitleBullets, "Salg", &seven),
            })
            .unwrap_err();
        assert!(matches!(&error, DeckOpError::Invalid { path, .. } if path == "slides[0].bullets"), "{error}");

        let title = "t".repeat(MAX_TITLE_CHARS + 1);
        let error = deck
            .apply(DeckOp::SetSlide {
                id: "s1".to_owned(),
                slide: slide(SlideLayout::TitleBullets, &title, &[]),
            })
            .unwrap_err();
        assert!(matches!(&error, DeckOpError::Invalid { path, .. } if path == "slides[0].title"), "{error}");

        let mut climbing = slide(SlideLayout::Image, "Bilde", &[]);
        climbing.image = Some("../etc/passwd".to_owned());
        let error = deck
            .apply(DeckOp::SetSlide { id: "s1".to_owned(), slide: climbing })
            .unwrap_err();
        assert!(matches!(&error, DeckOpError::Invalid { path, .. } if path == "slides[0].image"), "{error}");

        let error = deck
            .apply(DeckOp::SetSlide {
                id: "s1".to_owned(),
                slide: slide(SlideLayout::TwoColumn, "To kolonner", &["ikke plassert"]),
            })
            .unwrap_err();
        assert!(matches!(&error, DeckOpError::Invalid { path, .. } if path == "slides[0].bullets"), "{error}");
        assert_eq!(deck, before);
    }

    #[test]
    fn unknown_layout_is_refused() {
        let error = serde_json::from_value::<DeckOp>(json!({
            "op": "insert_slide",
            "slide": {"layout": "three_column", "title": "Tre"}
        }))
        .unwrap_err();
        assert!(error.to_string().contains("three_column"), "{error}");
    }

    #[test]
    fn move_to_missing_anchor_is_refused() {
        let mut deck = created(vec![
            slide(SlideLayout::Title, "En", &[]),
            slide(SlideLayout::Title, "To", &[]),
        ]);
        let before = deck.clone();
        assert_eq!(
            deck.apply(DeckOp::MoveSlide {
                id: "s1".to_owned(),
                after_id: Some("s9".to_owned()),
            }),
            Err(DeckOpError::UnknownSlide("s9".to_owned()))
        );
        assert!(deck
            .apply(DeckOp::MoveSlide {
                id: "s1".to_owned(),
                after_id: Some("s1".to_owned()),
            })
            .is_err());
        assert_eq!(deck, before);
    }

    #[test]
    fn canonical_json_is_stable() {
        let deck = DeckManifest::from_untrusted_json(include_str!("../tests/fixtures/deck_manifest_v1.json"))
            .expect("the fixture is a valid manifest");
        let first = deck.canonical_json();
        let reparsed = DeckManifest::from_untrusted_json(&first).unwrap();
        assert_eq!(reparsed, deck);
        assert_eq!(reparsed.canonical_json(), first);
        assert!(first.starts_with(r#"{"version":1,"deck_id":"#), "{first}");
    }

    #[test]
    fn from_untrusted_json_rejects_unknown_state() {
        let fixture = include_str!("../tests/fixtures/deck_manifest_v1.json");
        let mut value: Value = serde_json::from_str(fixture).unwrap();
        value["slides"][0]["state"] = json!("verified");
        assert!(DeckManifest::from_untrusted_json(&value.to_string()).is_err());

        let mut value: Value = serde_json::from_str(fixture).unwrap();
        value["slides"][0]["owner"] = json!("model");
        assert!(DeckManifest::from_untrusted_json(&value.to_string()).is_err());

        // An id the deck never minted is refused, so a stored deck cannot smuggle one in.
        let mut value: Value = serde_json::from_str(fixture).unwrap();
        value["slides"][0]["id"] = json!("s99");
        assert!(DeckManifest::from_untrusted_json(&value.to_string()).is_err());
    }

    #[test]
    fn builder_view_carries_only_manifest_v1_fields() {
        let deck = DeckManifest::from_untrusted_json(include_str!("../tests/fixtures/deck_manifest_v1.json")).unwrap();
        let view: Value = serde_json::from_str(&deck.builder_json()).unwrap();
        let mut top: Vec<&str> = view.as_object().unwrap().keys().map(String::as_str).collect();
        top.sort_unstable();
        assert_eq!(top, ["locale", "slides", "title", "version"]);
        for slide in view["slides"].as_array().unwrap() {
            for key in slide.as_object().unwrap().keys() {
                assert!(
                    ["id", "layout", "title", "bullets", "left", "right", "image", "notes"].contains(&key.as_str()),
                    "the builder would flag {key} as an unknown field"
                );
            }
        }
    }

    #[test]
    fn report_sets_states_and_is_capped_and_scrubbed() {
        let mut deck = created(vec![
            slide(SlideLayout::TitleBullets, "En", &["a"]),
            slide(SlideLayout::TitleBullets, "To", &["b"]),
            slide(SlideLayout::TitleBullets, "Tre", &["c"]),
        ]);
        let many: Vec<Value> = (0..20)
            .map(|n| json!({"id": format!("Text_May_Overflow\u{7}{n}"), "message": format!("line\nbreak\u{1b}[31m {}", "m".repeat(400))}))
            .collect();
        let report = BuildReport::from_untrusted_value(&json!({
            "deck": {"slides": 3, "file": "deck.pptx"},
            "slides": [
                {"id": "s1", "index": 1, "state": "ok", "findings": many},
                {"id": "s2", "index": 2, "state": "verified", "findings": []}
            ]
        }))
        .unwrap();
        deck.apply_report(&report);
        let first = &deck.slides[0];
        assert_eq!(first.state, SlideState::Warn, "ok with findings is warn");
        assert_eq!(first.findings.len(), MAX_FINDINGS);
        for finding in &first.findings {
            assert!(finding.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
            assert!(finding.message.chars().count() <= MAX_FINDING_MESSAGE_CHARS);
            assert!(!finding.message.chars().any(char::is_control));
        }
        assert_eq!(deck.slides[1].state, SlideState::Error, "an unknown state reads as error");
        assert_eq!(deck.slides[2].state, SlideState::Error, "an unreported slide is error");
        assert!(deck.validate().is_ok(), "a capped report keeps the manifest valid");

        deck.apply_report(&BuildReport::from_untrusted_value(&json!({
            "deck": {"slides": 0, "file": null},
            "slides": [],
            "refused": {"id": "theme_unsupported", "message": "only the default theme"}
        })).unwrap());
        assert!(deck.slides.iter().all(|slide| slide.state == SlideState::Error
            && slide.findings[0].id == "theme_unsupported"));
    }

    #[test]
    fn a_changed_slide_goes_back_to_draft() {
        let mut deck = created(vec![slide(SlideLayout::TitleBullets, "En", &["a"])]);
        deck.apply_report(&BuildReport::from_untrusted_value(&json!({
            "slides": [{"id": "s1", "state": "ok", "findings": []}]
        })).unwrap());
        assert_eq!(deck.slides[0].state, SlideState::Ok);
        deck.apply(DeckOp::SetSlide {
            id: "s1".to_owned(),
            slide: slide(SlideLayout::TitleBullets, "En", &["kortere"]),
        })
        .unwrap();
        assert_eq!(deck.slides[0].state, SlideState::Draft);
        assert!(deck.slides[0].findings.is_empty());
    }

    fn source() -> DeckSource {
        DeckSource {
            artifact_id: "research-01req".to_owned(),
            version: 2,
            provenance_digest: Some("a".repeat(64)),
            ledger_digest: Some("0123456789abcdef".repeat(4)),
            sources: vec![DeckCitation {
                number: 1,
                title: "SSB: Konsumprisindeksen".to_owned(),
                url: Some("https://www.ssb.no/kpi".to_owned()),
                document_id: None,
            }],
            unsupported_claims: vec!["Prisene faller neste år".to_owned()],
        }
    }

    #[test]
    fn a_deck_without_a_source_writes_no_source_key() {
        let deck = DeckManifest::from_untrusted_json(include_str!("../tests/fixtures/deck_manifest_v1.json")).unwrap();
        assert!(deck.source.is_none());
        assert!(!deck.canonical_json().contains("\"source\""));
        let view: Value = serde_json::from_str(&deck.builder_json()).unwrap();
        assert!(view.get("sources").is_none() && view.get("unsupported_claims").is_none());
    }

    #[test]
    fn a_source_survives_storage_and_reaches_the_builder() {
        let mut deck = DeckManifest::empty("deck-q3");
        deck.source = Some(source());
        deck.apply(DeckOp::Create {
            title: "Priser".to_owned(),
            locale: "nb-NO".to_owned(),
            slides: vec![slide(SlideLayout::TitleBullets, "KPI", &["Opp 3 % [1]"])],
        })
        .unwrap();
        let stored = deck.canonical_json();
        let reread = DeckManifest::from_untrusted_json(&stored).unwrap();
        assert_eq!(reread.source, Some(source()));
        assert_eq!(reread.canonical_json(), stored);
        // The marker stays in the manifest; the builder moves it into the notes.
        assert_eq!(reread.slides[0].bullets, ["Opp 3 % [1]"]);
        let view: Value = serde_json::from_str(&reread.builder_json()).unwrap();
        assert_eq!(view["sources"][0]["number"], 1);
        assert_eq!(view["unsupported_claims"][0], "Prisene faller neste år");
        assert!(view.get("source").is_none(), "the digests are not layout input");
    }

    #[test]
    fn a_bad_source_is_refused_with_its_field_path() {
        let cases: [(fn(&mut DeckSource), &str); 6] = [
            (|s| s.version = 0, "source.version"),
            (|s| s.provenance_digest = Some("ABC".to_owned()), "source.provenance_digest"),
            (|s| s.sources[0].number = 0, "source.sources[0].number"),
            (|s| s.sources.push(s.sources[0].clone()), "source.sources[1].number"),
            (|s| s.sources[0].title = "line\nbreak".to_owned(), "source.sources[0].title"),
            (|s| s.unsupported_claims.push(String::new()), "source.unsupported_claims[1]"),
        ];
        for (spoil, field) in cases {
            let mut bad = source();
            spoil(&mut bad);
            let mut deck = DeckManifest::empty("deck-q3");
            deck.source = Some(bad);
            let error = deck
                .apply(DeckOp::Create {
                    title: "Priser".to_owned(),
                    locale: "nb-NO".to_owned(),
                    slides: Vec::new(),
                })
                .unwrap_err();
            assert!(matches!(&error, DeckOpError::Invalid { path, .. } if path == field), "{field}: {error}");
        }
    }

    /// The builder holds the same caps in Python. Read as text, because the two
    /// live in different languages and the compiler cannot see the script.
    #[test]
    fn caps_match_the_builder() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../skills/office/pptx/scripts/build_deck.py");
        let script = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {} ({error}); re-point this test", path.display()));
        for (name, value) in [
            ("MANIFEST_VERSION", MANIFEST_VERSION as usize),
            ("MAX_MANIFEST_CHARS", MAX_MANIFEST_CHARS),
            ("MAX_SLIDES", MAX_SLIDES),
            ("MAX_BULLETS", MAX_BULLETS),
            ("MAX_BULLET_CHARS", MAX_BULLET_CHARS),
            ("MAX_TITLE_CHARS", MAX_TITLE_CHARS),
            ("MAX_NOTES_CHARS", MAX_NOTES_CHARS),
            ("MAX_ID_CHARS", MAX_ID_CHARS),
            ("MAX_SOURCES", MAX_SOURCES),
            ("MAX_CITATION_NUMBER", MAX_CITATION_NUMBER as usize),
            ("MAX_UNSUPPORTED_CLAIMS", MAX_UNSUPPORTED_CLAIMS),
        ] {
            let prefix = format!("{name} = ");
            let held = script
                .lines()
                .find_map(|line| line.strip_prefix(prefix.as_str()))
                .unwrap_or_else(|| panic!("build_deck.py no longer defines {name}; keep the caps in step"));
            assert_eq!(
                held.trim().replace('_', "").parse::<usize>().ok(),
                Some(value),
                "build_deck.py holds {name} = {held}, the contract {value}"
            );
        }
    }
}
