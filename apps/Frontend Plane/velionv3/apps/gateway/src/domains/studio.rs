//! Studio project persistence for the Velion v3 canvas.
//!
//! This is a narrow gateway-local repository for canvas projects and blocks.
//! It enforces the same session-derived org scope as the rest of the v3
//! gateway and delegates Social draft creation to the existing social domain.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    config::AppState,
    domains::social::{create_studio_social_draft, StudioSocialDraftInput},
    envelope::{error, ok},
    middleware::{require_session, AuthenticatedUser},
};

const DEFAULT_PROJECT_ID: &str = "studio_project_default";
const MAX_BLOCKS: usize = 80;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/studio/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/v1/studio/projects/:id",
            get(get_project).put(update_project),
        )
        .route(
            "/api/v1/studio/projects/:id/export/social-draft",
            post(export_social_draft),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Clone, Default)]
pub(crate) struct StudioStore {
    inner: Arc<RwLock<StudioStoreInner>>,
}

#[derive(Default)]
struct StudioStoreInner {
    projects_by_scope: BTreeMap<String, Vec<StudioProject>>,
    next_project_id: u64,
}

#[derive(Clone, Debug)]
struct StudioScope {
    org_id: String,
    user_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioProject {
    id: String,
    org_id: String,
    owner_user_id: String,
    updated_by_user_id: String,
    title: String,
    status: &'static str,
    blocks: Vec<StudioBlock>,
    selected_block_id: Option<String>,
    social_draft_id: Option<String>,
    social_exported_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioBlock {
    id: String,
    kind: String,
    title: String,
    body: Option<String>,
    image_url: Option<String>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectList {
    projects: Vec<StudioProject>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectMutation {
    project: StudioProject,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialDraftExport {
    project: StudioProject,
    social_post: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectBody {
    title: Option<String>,
    blocks: Option<Vec<StudioBlock>>,
    selected_block_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProjectBody {
    title: Option<String>,
    blocks: Option<Vec<StudioBlock>>,
    selected_block_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSocialDraftBody {
    title: Option<String>,
    body: Option<String>,
    platforms: Option<Vec<String>>,
    scheduled_at: Option<String>,
}

impl StudioStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    async fn list_projects(&self, scope: &StudioScope) -> Vec<StudioProject> {
        let mut inner = self.inner.write().await;
        seed_scope_projects(&mut inner, scope);
        inner
            .projects_by_scope
            .get(&scope_key(scope))
            .cloned()
            .unwrap_or_default()
    }

    async fn get_project(&self, scope: &StudioScope, id: &str) -> Option<StudioProject> {
        let mut inner = self.inner.write().await;
        seed_scope_projects(&mut inner, scope);
        inner
            .projects_by_scope
            .get(&scope_key(scope))?
            .iter()
            .find(|project| project.id == id)
            .cloned()
    }

    async fn create_project(
        &self,
        scope: &StudioScope,
        title: String,
        blocks: Vec<StudioBlock>,
        selected_block_id: Option<String>,
        now: DateTime<Utc>,
    ) -> StudioProject {
        let mut inner = self.inner.write().await;
        inner.next_project_id = inner.next_project_id.saturating_add(1);
        let now = now.to_rfc3339();
        let project = StudioProject {
            id: format!("studio_project_{}", inner.next_project_id),
            org_id: scope.org_id.clone(),
            owner_user_id: scope.user_id.clone(),
            updated_by_user_id: scope.user_id.clone(),
            title,
            status: "draft",
            blocks,
            selected_block_id,
            social_draft_id: None,
            social_exported_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let projects = inner.projects_by_scope.entry(scope_key(scope)).or_default();
        projects.insert(0, project.clone());
        project
    }

    async fn replace_project(
        &self,
        scope: &StudioScope,
        project: StudioProject,
    ) -> Option<StudioProject> {
        let mut inner = self.inner.write().await;
        seed_scope_projects(&mut inner, scope);
        let projects = inner.projects_by_scope.get_mut(&scope_key(scope))?;
        let index = projects
            .iter()
            .position(|candidate| candidate.id == project.id)?;
        projects[index] = project.clone();
        Some(project)
    }
}

async fn list_projects(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let scope = match authorized_studio_scope(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };

    Json(ok(ProjectList {
        projects: state.studio_store.list_projects(&scope).await,
    }))
    .into_response()
}

async fn create_project(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<CreateProjectBody>,
) -> axum::response::Response {
    let scope = match authorized_studio_scope(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let title = match normalize_title(body.title.as_deref(), "Untitled Studio canvas") {
        Ok(title) => title,
        Err(message) => return validation_error(message),
    };
    let blocks = match normalize_blocks(body.blocks.unwrap_or_else(default_canvas_blocks)) {
        Ok(blocks) => blocks,
        Err(message) => return validation_error(message),
    };
    let selected_block_id = normalize_selected_block_id(body.selected_block_id, &blocks);

    let project = state
        .studio_store
        .create_project(&scope, title, blocks, selected_block_id, Utc::now())
        .await;

    (StatusCode::CREATED, Json(ok(ProjectMutation { project }))).into_response()
}

async fn get_project(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let scope = match authorized_studio_scope(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let Some(project) = state.studio_store.get_project(&scope, &id).await else {
        return not_found();
    };

    Json(ok(ProjectMutation { project })).into_response()
}

async fn update_project(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> axum::response::Response {
    let scope = match authorized_studio_scope(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let Some(current) = state.studio_store.get_project(&scope, &id).await else {
        return not_found();
    };

    let title = match body.title {
        Some(title) => match normalize_title(Some(&title), "Untitled Studio canvas") {
            Ok(title) => title,
            Err(message) => return validation_error(message),
        },
        None => current.title.clone(),
    };
    let blocks = match body.blocks {
        Some(blocks) => match normalize_blocks(blocks) {
            Ok(blocks) => blocks,
            Err(message) => return validation_error(message),
        },
        None => current.blocks.clone(),
    };
    let selected_block_id = body
        .selected_block_id
        .map(|value| normalize_selected_block_id(Some(value), &blocks))
        .unwrap_or_else(|| current.selected_block_id.clone());

    let project = StudioProject {
        id: current.id,
        org_id: current.org_id,
        owner_user_id: current.owner_user_id,
        updated_by_user_id: scope.user_id.clone(),
        title,
        status: current.status,
        blocks,
        selected_block_id,
        social_draft_id: current.social_draft_id,
        social_exported_at: current.social_exported_at,
        created_at: current.created_at,
        updated_at: Utc::now().to_rfc3339(),
    };
    let Some(project) = state.studio_store.replace_project(&scope, project).await else {
        return not_found();
    };

    Json(ok(ProjectMutation { project })).into_response()
}

async fn export_social_draft(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<ExportSocialDraftBody>,
) -> axum::response::Response {
    let scope = match authorized_studio_scope(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let Some(project) = state.studio_store.get_project(&scope, &id).await else {
        return not_found();
    };
    let draft = match build_social_draft_input(&project, body, Utc::now()) {
        Ok(input) => input,
        Err(message) => return validation_error(message),
    };
    let social_post = match create_studio_social_draft(&state, &user, &scope.org_id, draft).await {
        Ok(post) => post,
        Err(response) => return response,
    };
    let social_draft_id = social_post
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let exported_project = StudioProject {
        social_draft_id,
        social_exported_at: Some(Utc::now().to_rfc3339()),
        updated_by_user_id: scope.user_id.clone(),
        updated_at: Utc::now().to_rfc3339(),
        ..project
    };
    let project = match state
        .studio_store
        .replace_project(&scope, exported_project)
        .await
    {
        Some(project) => project,
        None => state
            .studio_store
            .get_project(&scope, &id)
            .await
            .unwrap_or_else(|| default_project(&scope, Utc::now())),
    };

    (
        StatusCode::CREATED,
        Json(ok(SocialDraftExport {
            project,
            social_post,
        })),
    )
        .into_response()
}

async fn authorized_studio_scope(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<StudioScope, axum::response::Response> {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "org_scope_required",
                "An authorized organization scope is required.",
            )),
        )
            .into_response());
    }

    Ok(StudioScope {
        org_id,
        user_id: user.user_id.clone(),
    })
}

fn build_social_draft_input(
    project: &StudioProject,
    body: ExportSocialDraftBody,
    now: DateTime<Utc>,
) -> Result<StudioSocialDraftInput, String> {
    let title = normalize_title(
        body.title.as_deref().or(Some(project.title.as_str())),
        &project.title,
    )?;
    let body_text = body
        .body
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| draft_body_from_project(project));
    if body_text.chars().count() > 5000 {
        return Err("body must be 5000 characters or fewer.".to_owned());
    }

    let platforms = normalize_social_platforms(body.platforms.as_deref())?;
    let scheduled_at = body
        .scheduled_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| (now + Duration::hours(24)).to_rfc3339());
    if DateTime::parse_from_rfc3339(&scheduled_at).is_err() {
        return Err("scheduledAt must be a valid RFC3339 timestamp.".to_owned());
    }

    Ok(StudioSocialDraftInput {
        project_id: project.id.clone(),
        title,
        body: body_text,
        platforms,
        scheduled_at,
        source_label: project.title.clone(),
    })
}

fn draft_body_from_project(project: &StudioProject) -> String {
    let lines = project
        .blocks
        .iter()
        .filter_map(|block| {
            let title = block.title.trim();
            let body = block.body.as_deref().unwrap_or_default().trim();
            match (title.is_empty(), body.is_empty()) {
                (true, true) => None,
                (false, true) => Some(title.to_owned()),
                (true, false) => Some(body.to_owned()),
                (false, false) => Some(format!("{title}: {body}")),
            }
        })
        .take(8)
        .collect::<Vec<_>>();

    if lines.is_empty() {
        format!("Draft social content for {}.", project.title)
    } else {
        lines.join("\n\n")
    }
}

fn normalize_blocks(blocks: Vec<StudioBlock>) -> Result<Vec<StudioBlock>, String> {
    if blocks.len() > MAX_BLOCKS {
        return Err(format!("projects can contain at most {MAX_BLOCKS} blocks."));
    }
    let blocks = blocks
        .into_iter()
        .map(normalize_block)
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = BTreeSet::new();
    if blocks.iter().any(|block| !ids.insert(block.id.as_str())) {
        return Err("block ids must be unique.".to_owned());
    }
    Ok(blocks)
}

fn normalize_block(block: StudioBlock) -> Result<StudioBlock, String> {
    let id = block.id.trim().to_owned();
    if id.is_empty() || id.chars().count() > 120 {
        return Err("block id is required and must be 120 characters or fewer.".to_owned());
    }
    if !is_allowed_block_kind(&block.kind) {
        return Err("block kind is not supported.".to_owned());
    }
    let title = block.title.trim().to_owned();
    if title.is_empty() || title.chars().count() > 160 {
        return Err("block title is required and must be 160 characters or fewer.".to_owned());
    }
    if block
        .body
        .as_deref()
        .map(|value| value.chars().count() > 5000)
        .unwrap_or(false)
    {
        return Err("block body must be 5000 characters or fewer.".to_owned());
    }
    if !valid_dimension(block.x, 0.0, 5000.0)
        || !valid_dimension(block.y, 0.0, 5000.0)
        || !valid_dimension(block.width, 24.0, 2000.0)
        || !valid_dimension(block.height, 24.0, 2000.0)
    {
        return Err("block geometry is outside the supported canvas range.".to_owned());
    }

    Ok(StudioBlock {
        id,
        kind: block.kind,
        title,
        body: block.body.and_then(trimmed_optional),
        image_url: block.image_url.and_then(trimmed_optional),
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
    })
}

fn normalize_title(value: Option<&str>, fallback: &str) -> Result<String, String> {
    let title = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned();
    if title.chars().count() > 140 {
        return Err("title must be 140 characters or fewer.".to_owned());
    }
    Ok(title)
}

fn normalize_selected_block_id(
    selected_block_id: Option<String>,
    blocks: &[StudioBlock],
) -> Option<String> {
    let selected = selected_block_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    blocks
        .iter()
        .any(|block| block.id == selected)
        .then(|| selected.to_owned())
}

fn normalize_social_platforms(platforms: Option<&[String]>) -> Result<Vec<&'static str>, String> {
    let raw = platforms
        .map(|values| values.iter().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["linkedin", "x"]);
    let mut normalized = Vec::new();
    for platform in raw {
        let Some(platform) = social_platform(platform.trim()) else {
            return Err(
                "platforms can include linkedin, x, instagram, facebook, tiktok, or snapchat."
                    .to_owned(),
            );
        };
        if !normalized.contains(&platform) {
            normalized.push(platform);
        }
    }
    if normalized.is_empty() {
        return Err("at least one platform is required.".to_owned());
    }
    Ok(normalized)
}

fn social_platform(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "linkedin" => Some("linkedin"),
        "x" | "twitter" => Some("x"),
        "instagram" => Some("instagram"),
        "facebook" => Some("facebook"),
        "tiktok" | "tik_tok" => Some("tiktok"),
        "snapchat" => Some("snapchat"),
        _ => None,
    }
}

fn seed_scope_projects(inner: &mut StudioStoreInner, scope: &StudioScope) {
    inner
        .projects_by_scope
        .entry(scope_key(scope))
        .or_insert_with(|| vec![default_project(scope, Utc::now())]);
}

fn default_project(scope: &StudioScope, now: DateTime<Utc>) -> StudioProject {
    let now = now.to_rfc3339();
    StudioProject {
        id: DEFAULT_PROJECT_ID.to_owned(),
        org_id: scope.org_id.clone(),
        owner_user_id: scope.user_id.clone(),
        updated_by_user_id: scope.user_id.clone(),
        title: "Launch canvas".to_owned(),
        status: "draft",
        blocks: default_canvas_blocks(),
        selected_block_id: Some("profile".to_owned()),
        social_draft_id: None,
        social_exported_at: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn default_canvas_blocks() -> Vec<StudioBlock> {
    vec![
        StudioBlock {
            id: "profile".to_owned(),
            kind: "profile".to_owned(),
            title: "Ava Berg".to_owned(),
            body: Some("Creative lead, Velion".to_owned()),
            image_url: Some("https://images.unsplash.com/photo-1494790108377-be9c29b29330?auto=format&fit=crop&w=420&q=80".to_owned()),
            x: 92.0,
            y: 82.0,
            width: 390.0,
            height: 300.0,
        },
        StudioBlock {
            id: "positioning".to_owned(),
            kind: "text".to_owned(),
            title: "Campaign hook".to_owned(),
            body: Some("Turn support signals into public trust. Show the workflow, not the promise.".to_owned()),
            image_url: None,
            x: 520.0,
            y: 92.0,
            width: 310.0,
            height: 210.0,
        },
        StudioBlock {
            id: "brand".to_owned(),
            kind: "brand".to_owned(),
            title: "VELION".to_owned(),
            body: Some("Quiet operations, visible momentum".to_owned()),
            image_url: None,
            x: 868.0,
            y: 90.0,
            width: 330.0,
            height: 220.0,
        },
        StudioBlock {
            id: "workspace".to_owned(),
            kind: "image".to_owned(),
            title: "Product workspace".to_owned(),
            body: Some("Dashboard crop for launch story".to_owned()),
            image_url: Some("https://images.unsplash.com/photo-1497366811353-6870744d04b2?auto=format&fit=crop&w=520&q=80".to_owned()),
            x: 1252.0,
            y: 90.0,
            width: 320.0,
            height: 590.0,
        },
        StudioBlock {
            id: "motion".to_owned(),
            kind: "image".to_owned(),
            title: "Motion background".to_owned(),
            body: Some("Use as short-form opening scene".to_owned()),
            image_url: Some("https://images.unsplash.com/photo-1500530855697-b586d89ba3ee?auto=format&fit=crop&w=780&q=80".to_owned()),
            x: 92.0,
            y: 420.0,
            width: 690.0,
            height: 270.0,
        },
        StudioBlock {
            id: "x-card".to_owned(),
            kind: "link".to_owned(),
            title: "@velion on X".to_owned(),
            body: Some("x.com/velion".to_owned()),
            image_url: None,
            x: 870.0,
            y: 350.0,
            width: 320.0,
            height: 265.0,
        },
    ]
}

fn scope_key(scope: &StudioScope) -> String {
    format!("{}:{}", scope.org_id, scope.user_id)
}

fn is_allowed_block_kind(kind: &str) -> bool {
    matches!(
        kind,
        "profile" | "image" | "text" | "brand" | "video" | "link" | "social"
    )
}

fn valid_dimension(value: f64, min: f64, max: f64) -> bool {
    value.is_finite() && value >= min && value <= max
}

fn trimmed_optional(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn validation_error(message: String) -> axum::response::Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(error("validation_error", message)),
    )
        .into_response()
}

fn not_found() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "studio_project_not_found",
            "Studio project not found.",
        )),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(org_id: &str, user_id: &str) -> StudioScope {
        StudioScope {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
        }
    }

    #[tokio::test]
    async fn studio_store_scopes_projects_by_org_and_user() {
        let store = StudioStore::new();
        let user_a = scope("org_a", "user_a");
        let user_b = scope("org_a", "user_b");
        let projects = store.list_projects(&user_a).await;
        assert_eq!(projects.len(), 1);

        let created = store
            .create_project(
                &user_a,
                "User A canvas".to_owned(),
                default_canvas_blocks(),
                Some("profile".to_owned()),
                Utc::now(),
            )
            .await;
        assert_eq!(created.owner_user_id, "user_a");

        let user_a_projects = store.list_projects(&user_a).await;
        assert_eq!(user_a_projects.len(), 2);
        assert_eq!(
            user_a_projects
                .first()
                .map(|project| project.title.as_str()),
            Some("User A canvas")
        );

        let user_b_projects = store.list_projects(&user_b).await;
        assert_eq!(user_b_projects.len(), 1);
        assert_eq!(
            user_b_projects
                .first()
                .map(|project| project.title.as_str()),
            Some("Launch canvas")
        );
    }

    #[test]
    fn social_draft_input_uses_canvas_blocks_and_deduplicates_platforms() {
        let project = default_project(&scope("org_a", "user_a"), Utc::now());
        let input = build_social_draft_input(
            &project,
            ExportSocialDraftBody {
                title: None,
                body: None,
                platforms: Some(vec![
                    "linkedin".to_owned(),
                    "twitter".to_owned(),
                    "x".to_owned(),
                ]),
                scheduled_at: Some("2026-06-20T10:00:00.000Z".to_owned()),
            },
            Utc::now(),
        )
        .expect("valid draft input");

        assert_eq!(input.title, "Launch canvas");
        assert_eq!(input.platforms, vec!["linkedin", "x"]);
        assert!(input.body.contains("Campaign hook"));
        assert_eq!(input.project_id, DEFAULT_PROJECT_ID);
    }

    #[test]
    fn rejects_invalid_block_geometry() {
        let mut block = default_canvas_blocks().remove(0);
        block.width = f64::NAN;

        assert_eq!(
            normalize_blocks(vec![block]).unwrap_err(),
            "block geometry is outside the supported canvas range."
        );
    }

    #[test]
    fn rejects_duplicate_block_ids() {
        let blocks = vec![
            StudioBlock {
                id: "duplicate".to_owned(),
                kind: "text".to_owned(),
                title: "First".to_owned(),
                body: None,
                image_url: None,
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 180.0,
            },
            StudioBlock {
                id: "duplicate".to_owned(),
                kind: "text".to_owned(),
                title: "Second".to_owned(),
                body: None,
                image_url: None,
                x: 320.0,
                y: 0.0,
                width: 300.0,
                height: 180.0,
            },
        ];

        assert_eq!(
            normalize_blocks(blocks).unwrap_err(),
            "block ids must be unique."
        );
    }
}
