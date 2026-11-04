//! Brand deck templates for the template picker (Wave 3 EP-5 S7).
//!
//! A deck template is an org skill bundle that carries `templates/deck-theme.json` (EP-5 decision D4:
//! templates are capability-core skill bundles, not Data Plane documents). The code calls them "deck
//! themes" where it could be confused with EP-11's composer templates (integration round B, C-B7).
//!
//! Everything here reads capability-core under the caller's own verified capability bearer, so
//! capability-core takes the organization from that bearer and no org from a header, a query or a body
//! is ever named. The gateway's skill store then keeps only the skills this user may use. What leaves is
//! display data alone: the theme's name, description, colours and font names. A blob, a logo or a master
//! never does.
//!
//! A template is chosen by id, never by keyword (decision D5): the picker sends the skill id as the
//! invoke field `deck_template`, and [`resolve`] checks it against the same listing before a turn may use
//! it, so the model never names a bundle.
//!
//! Off by default: `SKILL_DECK_TEMPLATES_ENABLED`, the literal `true` only. Off, `GET /v1/deck-templates`
//! answers 404 as if it did not exist and an invoke's `deck_template` is never read.

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::state::AppState;

/// The switch for the template picker and the `deck_template` invoke field.
pub const DECK_TEMPLATES_ENV: &str = "SKILL_DECK_TEMPLATES_ENABLED";

/// The bundle path that makes a skill bundle a deck template.
pub const THEME_PATH: &str = "templates/deck-theme.json";

/// The only theme contract version this reader knows.
pub const THEME_VERSION: u64 = 1;

/// Most templates one listing returns.
pub const MAX_TEMPLATES: usize = 32;

/// Most bundles one listing opens: each costs a manifest read and a blob read, so an org with many
/// bundles cannot turn one picker load into hundreds of capability-core calls.
pub const MAX_BUNDLES_SCANNED: usize = 64;

/// Largest theme file read. A theme is a few hundred bytes; anything far larger is not one.
pub const MAX_THEME_BYTES: i64 = 32 * 1024;

/// Longest skill id accepted as a template id.
pub const MAX_SKILL_ID_CHARS: usize = 128;

const MAX_NAME_CHARS: usize = 80;
const MAX_DESCRIPTION_CHARS: usize = 200;
const MAX_FONT_CHARS: usize = 64;
const MAX_ACCENTS: usize = 4;

/// Whether `raw` (the value of [`DECK_TEMPLATES_ENV`], if set) is the literal on-word.
#[must_use]
pub fn enabled_from(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether deck templates are switched on in this process.
#[must_use]
pub fn enabled() -> bool {
    enabled_from(std::env::var(DECK_TEMPLATES_ENV).ok().as_deref())
}

/// A template's colours, each a lower-case `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeckTemplateColors {
    pub primary: String,
    pub secondary: String,
    pub text: String,
    pub background: String,
    pub accent: Vec<String>,
}

/// A template's font family names, as the theme names them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeckTemplateFonts {
    pub heading: String,
    pub body: String,
}

/// One deck template as the picker shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeckTemplate {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub colors: DeckTemplateColors,
    pub fonts: DeckTemplateFonts,
}

/// The display half of a theme file. Unknown keys (logo, master, footer text) are the builder's and
/// are ignored here, not refused.
#[derive(Debug, Deserialize)]
struct RawTheme {
    version: u64,
    name: String,
    #[serde(default)]
    description: Option<String>,
    colors: RawColors,
    fonts: RawFonts,
}

#[derive(Debug, Deserialize)]
struct RawColors {
    primary: String,
    secondary: String,
    text: String,
    background: String,
    #[serde(default)]
    accent: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawFonts {
    heading: String,
    body: String,
}

/// Whether `id` can be a template id: 1 to [`MAX_SKILL_ID_CHARS`] of `[A-Za-z0-9_.:-]`. Checked before
/// any read, so a crafted value never reaches capability-core.
#[must_use]
pub fn valid_skill_id(id: &str) -> bool {
    (1..=MAX_SKILL_ID_CHARS).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

/// The template an invoke asked for: the trimmed `deck_template` string, or `None` when it is absent,
/// blank or not a string. Shape only; [`resolve`] decides whether the caller may use it.
#[must_use]
pub fn requested(raw: Option<&serde_json::Value>) -> Option<&str> {
    raw.and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// The zero-retention feature a template refusal names. No key of its own: a ZDR turn never mounts a
/// bundle, so picking a template is refused exactly like naming a skill (`zdr-refusals.json` unchanged).
pub const ZDR_FEATURE: &str = "skills";

/// Whether a zero-retention turn must be refused for its `deck_template`: only while the switch is on,
/// so with it off a body carrying the field is treated exactly as before the field existed.
#[must_use]
pub fn zdr_refused(enabled: bool, raw: Option<&serde_json::Value>) -> bool {
    enabled && requested(raw).is_some()
}

/// A colour as the theme contract writes it, `#rrggbb`, lower-cased; anything else is `None`.
fn hex_colour(raw: &str) -> Option<String> {
    let value = raw.trim();
    let digits = value.strip_prefix('#')?;
    (digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

/// Theme text as display text: control characters dropped, trimmed, at most `max` characters.
fn display_text(raw: &str, max: usize) -> String {
    raw.chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

/// A font family name, or `None` when it is blank or too long to be one. A name the sandbox lacks is
/// still listed: the builder reports it as substituted (EP-5 S6), the picker only shows it.
fn font_name(raw: &str) -> Option<String> {
    let name = display_text(raw, MAX_FONT_CHARS + 1);
    (!name.is_empty() && name.chars().count() <= MAX_FONT_CHARS).then_some(name)
}

/// The picker's view of a theme file, or `None` when the bytes are not a version-1 theme with a name,
/// four hex colours and two font names. Accent colours past [`MAX_ACCENTS`] or not hex are dropped.
#[must_use]
pub fn template_from_theme(skill_id: &str, bytes: &[u8]) -> Option<DeckTemplate> {
    let raw: RawTheme = serde_json::from_slice(bytes).ok()?;
    if raw.version != THEME_VERSION {
        return None;
    }
    let name = display_text(&raw.name, MAX_NAME_CHARS);
    if name.is_empty() {
        return None;
    }
    let colors = DeckTemplateColors {
        primary: hex_colour(&raw.colors.primary)?,
        secondary: hex_colour(&raw.colors.secondary)?,
        text: hex_colour(&raw.colors.text)?,
        background: hex_colour(&raw.colors.background)?,
        accent: raw
            .colors
            .accent
            .iter()
            .filter_map(|colour| hex_colour(colour))
            .take(MAX_ACCENTS)
            .collect(),
    };
    let fonts = DeckTemplateFonts {
        heading: font_name(&raw.fonts.heading)?,
        body: font_name(&raw.fonts.body)?,
    };
    Some(DeckTemplate {
        skill_id: skill_id.to_owned(),
        name,
        description: raw
            .description
            .as_deref()
            .map(|text| display_text(text, MAX_DESCRIPTION_CHARS))
            .unwrap_or_default(),
        colors,
        fonts,
    })
}

/// The bundles whose skill this user may use in this org, in listing order, at most
/// [`MAX_BUNDLES_SCANNED`]. capability-core already scoped the listing to the bearer's org; this also
/// drops a bundle whose skill the gateway does not hold for `org_id`, or holds as another user's own.
#[must_use]
pub fn usable_skill_ids(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    bundles: &[mp_contracts::model_plane::v1::SkillBundleSummary],
) -> Vec<String> {
    use mp_contracts::model_plane::v1::GetSkillRequest;

    bundles
        .iter()
        .filter(|bundle| valid_skill_id(&bundle.skill_id))
        .filter(|bundle| {
            crate::skills::handle_get_skill(
                &state.skills,
                GetSkillRequest {
                    request_id: String::new(),
                    org_id: org_id.to_owned(),
                    skill_id: bundle.skill_id.clone(),
                },
                &state.ownership,
                user_id,
            )
            .is_ok()
        })
        .map(|bundle| bundle.skill_id.clone())
        .take(MAX_BUNDLES_SCANNED)
        .collect()
}

/// Picker order: by name, case-insensitively, then by skill id so equal names stay stable; at most
/// [`MAX_TEMPLATES`].
fn picker_order(mut templates: Vec<DeckTemplate>) -> Vec<DeckTemplate> {
    templates.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.skill_id.cmp(&b.skill_id))
    });
    templates.truncate(MAX_TEMPLATES);
    templates
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("writing to a String");
            hex
        })
}

/// The theme of one bundle, or `None` when the bundle has no theme file, the file is binary or too
/// large, its stored bytes do not match the manifest digest, or it is not a valid theme. Advisory per
/// bundle: one bad bundle costs that template, never the listing.
async fn template_for_skill(state: &AppState, bearer: &str, skill_id: &str) -> Option<DeckTemplate> {
    use mp_contracts::model_plane::v1::{GetSkillBlobRequest, GetSkillBundleManifestRequest};

    let manifest = match state
        .capability_client
        .clone()
        .get_skill_bundle_manifest(crate::tool_loop::with_authorization(
            GetSkillBundleManifestRequest {
                skill_id: skill_id.to_owned(),
            },
            bearer,
        ))
        .await
    {
        Ok(response) => response.into_inner(),
        Err(status) => {
            if status.code() != tonic::Code::NotFound {
                tracing::warn!(code = ?status.code(), "deck templates: bundle manifest read failed");
            }
            return None;
        }
    };
    let file = manifest.files.iter().find(|file| file.path == THEME_PATH)?;
    if file.binary || !(1..=MAX_THEME_BYTES).contains(&file.size_bytes) {
        return None;
    }
    let bytes = match state
        .capability_client
        .clone()
        .get_skill_blob(crate::tool_loop::with_authorization(
            GetSkillBlobRequest {
                skill_id: skill_id.to_owned(),
                sha256: file.sha256.clone(),
            },
            bearer,
        ))
        .await
    {
        Ok(response) => response.into_inner().content,
        Err(status) => {
            tracing::warn!(code = ?status.code(), "deck templates: theme read failed");
            return None;
        }
    };
    if sha256_hex(&bytes) != file.sha256 {
        tracing::warn!("deck templates: theme bytes do not match the manifest digest; template skipped");
        return None;
    }
    template_from_theme(skill_id, &bytes)
}

/// The deck templates this user may pick, in picker order. Fails only when capability-core cannot list
/// the org's bundles at all; see [`template_for_skill`] for what drops a single template.
///
/// # Errors
/// The listing's own status when `ListSkillBundles` fails.
pub async fn list_for_caller(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    capability_bearer: &str,
) -> Result<Vec<DeckTemplate>, tonic::Status> {
    use mp_contracts::model_plane::v1::ListSkillBundlesRequest;

    let bundles = state
        .capability_client
        .clone()
        .list_skill_bundles(crate::tool_loop::with_authorization(
            ListSkillBundlesRequest {
                skill_ids: Vec::new(),
            },
            capability_bearer,
        ))
        .await?
        .into_inner()
        .bundles;
    let mut templates = Vec::new();
    for skill_id in usable_skill_ids(state, org_id, user_id, &bundles) {
        if let Some(template) = template_for_skill(state, capability_bearer, &skill_id).await {
            templates.push(template);
        }
    }
    Ok(picker_order(templates))
}

/// The template an invoke's `deck_template` names, checked the way the listing checks it: a valid id,
/// a bundle capability-core lists for the bearer's org, a skill this user may use, and a readable theme.
/// `None` for anything else, so a turn treats an id it may not use exactly like one that does not exist
/// and the model is only ever told the resolved name.
pub async fn resolve(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    capability_bearer: &str,
    skill_id: &str,
) -> Option<DeckTemplate> {
    use mp_contracts::model_plane::v1::ListSkillBundlesRequest;

    if !valid_skill_id(skill_id) || capability_bearer.trim().is_empty() {
        return None;
    }
    let bundles = match state
        .capability_client
        .clone()
        .list_skill_bundles(crate::tool_loop::with_authorization(
            ListSkillBundlesRequest {
                skill_ids: vec![skill_id.to_owned()],
            },
            capability_bearer,
        ))
        .await
    {
        Ok(response) => response.into_inner().bundles,
        Err(status) => {
            tracing::warn!(code = ?status.code(), "deck templates: bundle listing failed");
            return None;
        }
    };
    let listed = usable_skill_ids(state, org_id, user_id, &bundles);
    if !listed.iter().any(|id| id == skill_id) {
        return None;
    }
    template_for_skill(state, capability_bearer, skill_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::{Skill, SkillBundleSummary};

    fn theme() -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "name": "Aquatiq brand",
            "description": "Blue on white, for customer decks.",
            "colors": {
                "primary": "#0A3D62",
                "secondary": "#3C6382",
                "text": "#1E272E",
                "background": "#FFFFFF",
                "accent": ["#E58E26", "#B71540", "red", "#78E08F", "#60A3BC", "#F8C291"]
            },
            "fonts": { "heading": "Carlito", "body": "Liberation Sans" },
            "logo": "templates/logo.png",
            "master": "templates/master.pptx",
            "footer_text": "Aquatiq"
        })
    }

    fn bytes(value: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }

    fn bundle(skill_id: &str) -> SkillBundleSummary {
        SkillBundleSummary {
            skill_id: skill_id.to_owned(),
            manifest_sha256: "a".repeat(64),
            file_count: 2,
            total_bytes: 512,
            created_by: "admin-1".to_owned(),
            created_at: "2026-11-04T10:00:00Z".to_owned(),
            deck_template: false,
        }
    }

    fn skill(id: &str, name: &str) -> Skill {
        Skill {
            id: id.to_owned(),
            name: name.to_owned(),
            body: "Brand rules.".to_owned(),
            tags: Vec::new(),
            source_path: String::new(),
            min_score: 0.0,
        }
    }

    fn template(skill_id: &str, name: &str) -> DeckTemplate {
        let mut value = theme();
        value["name"] = serde_json::Value::from(name);
        template_from_theme(skill_id, &bytes(&value)).unwrap()
    }

    #[test]
    fn deck_templates_switch_is_the_literal_true_only() {
        assert!(enabled_from(Some("true")));
        for off in [None, Some(""), Some("TRUE"), Some("True"), Some("1"), Some("yes"), Some(" true")] {
            assert!(!enabled_from(off), "{off:?}");
        }
    }

    #[test]
    fn deck_templates_theme_gives_display_fields_only() {
        let parsed = template_from_theme("skill-brand", &bytes(&theme())).unwrap();
        assert_eq!(parsed.skill_id, "skill-brand");
        assert_eq!(parsed.name, "Aquatiq brand");
        assert_eq!(parsed.description, "Blue on white, for customer decks.");
        assert_eq!(parsed.colors.primary, "#0a3d62");
        assert_eq!(parsed.colors.background, "#ffffff");
        // A non-hex accent is dropped and the list stops at four.
        assert_eq!(parsed.colors.accent, vec!["#e58e26", "#b71540", "#78e08f", "#60a3bc"]);
        assert_eq!(parsed.fonts.heading, "Carlito");
        assert_eq!(parsed.fonts.body, "Liberation Sans");
        // The builder's keys never reach the picker.
        let wire = serde_json::to_value(&parsed).unwrap();
        for key in ["logo", "master", "footer_text", "version"] {
            assert!(wire.get(key).is_none(), "{key} leaked: {wire}");
        }
    }

    #[test]
    fn deck_templates_refuse_a_theme_that_is_not_one() {
        let mut bad_colour = theme();
        bad_colour["colors"]["primary"] = serde_json::Value::from("blue");
        let mut short_colour = theme();
        short_colour["colors"]["text"] = serde_json::Value::from("#fff");
        let mut wrong_version = theme();
        wrong_version["version"] = serde_json::Value::from(2);
        let mut blank_name = theme();
        blank_name["name"] = serde_json::Value::from("  \u{7}  ");
        let mut long_font = theme();
        long_font["fonts"]["body"] = serde_json::Value::from("F".repeat(MAX_FONT_CHARS + 1));
        let mut no_fonts = theme();
        no_fonts.as_object_mut().unwrap().remove("fonts");
        for (case, value) in [
            ("bad colour", bad_colour),
            ("short colour", short_colour),
            ("wrong version", wrong_version),
            ("blank name", blank_name),
            ("long font", long_font),
            ("no fonts", no_fonts),
        ] {
            assert!(template_from_theme("skill-brand", &bytes(&value)).is_none(), "{case}");
        }
        assert!(template_from_theme("skill-brand", b"not json").is_none());
    }

    #[test]
    fn deck_templates_cap_and_clean_display_text() {
        let mut value = theme();
        value["name"] = serde_json::Value::from(format!("  {}\u{0}\n", "N".repeat(200)));
        value["description"] = serde_json::Value::from("D".repeat(500));
        let parsed = template_from_theme("skill-brand", &bytes(&value)).unwrap();
        assert_eq!(parsed.name, "N".repeat(MAX_NAME_CHARS));
        assert_eq!(parsed.description.chars().count(), MAX_DESCRIPTION_CHARS);
        let mut no_description = theme();
        no_description.as_object_mut().unwrap().remove("description");
        assert_eq!(template_from_theme("skill-brand", &bytes(&no_description)).unwrap().description, "");
    }

    #[test]
    fn deck_templates_skill_id_shape() {
        for good in ["skill-brand", "01HZX3K9Q2", "org:brand.v2_1"] {
            assert!(valid_skill_id(good), "{good}");
        }
        let too_long = "x".repeat(MAX_SKILL_ID_CHARS + 1);
        for bad in ["", "a b", "../blob", "id/with/slash", "é", too_long.as_str()] {
            assert!(!valid_skill_id(bad), "{bad}");
        }
        let picked = serde_json::json!("  skill-brand ");
        assert_eq!(requested(Some(&picked)), Some("skill-brand"));
        assert_eq!(requested(Some(&serde_json::json!("   "))), None);
        assert_eq!(requested(Some(&serde_json::json!(42))), None);
        assert_eq!(requested(None), None);
    }

    #[test]
    fn deck_templates_zdr_refusal_only_while_on_and_only_for_a_pick() {
        let picked = serde_json::json!("skill-brand");
        assert!(zdr_refused(true, Some(&picked)));
        assert!(!zdr_refused(false, Some(&picked)), "off, the field is not read at all");
        assert!(!zdr_refused(true, Some(&serde_json::json!(""))));
        assert!(!zdr_refused(true, None));
        assert_eq!(ZDR_FEATURE, "skills");
    }

    #[tokio::test]
    async fn deck_templates_list_only_skills_of_this_org_the_user_may_use() {
        let state = AppState::new();
        state.skills.upsert("org-a", skill("skill-brand", "Brand"));
        state.skills.upsert("org-b", skill("skill-other-org", "Other org"));
        let bundles = vec![
            bundle("skill-brand"),
            bundle("skill-other-org"),
            bundle("skill-unknown"),
            bundle("../escape"),
        ];
        assert_eq!(usable_skill_ids(&state, "org-a", "user-a", &bundles), vec!["skill-brand"]);
        assert_eq!(usable_skill_ids(&state, "org-b", "user-a", &bundles), vec!["skill-other-org"]);
    }

    #[tokio::test]
    async fn deck_templates_scan_is_bounded() {
        let state = AppState::new();
        let bundles: Vec<SkillBundleSummary> = (0..MAX_BUNDLES_SCANNED + 10)
            .map(|index| {
                let id = format!("skill-{index}");
                state.skills.upsert("org-a", skill(&id, &id));
                bundle(&id)
            })
            .collect();
        assert_eq!(usable_skill_ids(&state, "org-a", "user-a", &bundles).len(), MAX_BUNDLES_SCANNED);
    }

    #[test]
    fn deck_templates_picker_order_is_by_name_then_id_and_capped() {
        let ordered = picker_order(vec![
            template("skill-c", "beta"),
            template("skill-b", "Alpha"),
            template("skill-a", "alpha"),
        ]);
        let ids: Vec<&str> = ordered.iter().map(|template| template.skill_id.as_str()).collect();
        assert_eq!(ids, vec!["skill-a", "skill-b", "skill-c"]);
        let many = (0..MAX_TEMPLATES + 5)
            .map(|index| template(&format!("skill-{index}"), &format!("T{index:03}")))
            .collect();
        assert_eq!(picker_order(many).len(), MAX_TEMPLATES);
    }

    #[test]
    fn deck_templates_digest_is_the_manifest_spelling() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn deck_templates_resolve_refuses_a_malformed_id_or_no_credential_before_any_read() {
        // Both are refused on shape alone, before capability-core is asked anything.
        let state = AppState::new();
        state.skills.upsert("org-a", skill("skill-brand", "Brand"));
        assert!(resolve(&state, "org-a", "user-a", "bearer", "../blob").await.is_none());
        assert!(resolve(&state, "org-a", "user-a", "  ", "skill-brand").await.is_none());
    }
}
