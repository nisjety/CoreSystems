use axum::{
    extract::{Extension, Path, State},
    http::Uri,
    response::IntoResponse,
};
use reqwest::Method;

use crate::{
    config::AppState, domains::knowledge::shared, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

pub(super) async fn list_wiki_pages(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/wiki/pages{}", state.wiki_store_url, shared::qs(&uri));
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn wiki_page_by_path(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/by-path{}",
        state.wiki_store_url,
        shared::qs(&uri)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn get_wiki_page(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}",
        state.wiki_store_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn wiki_page_versions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}/versions{}",
        state.wiki_store_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn wiki_page_diff(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}/diff{}",
        state.wiki_store_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn wiki_page_backlinks(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}/backlinks{}",
        state.wiki_store_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}
