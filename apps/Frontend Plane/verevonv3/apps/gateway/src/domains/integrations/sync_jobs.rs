use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::Response,
};
use reqwest::Method;

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared::{proxy_for_user, proxy_sse_for_user};

fn sync_job_events_url(integration_core_url: &str, id: &str) -> String {
    format!(
        "{}/api/v1/sync-jobs/{}/events",
        integration_core_url,
        urlencoding::encode(id)
    )
}

#[cfg(test)]
mod tests {
    use super::sync_job_events_url;

    #[test]
    fn sync_job_events_url_encodes_the_untrusted_job_identifier() {
        assert_eq!(
            sync_job_events_url("http://integration-core:3026", "sync/id"),
            "http://integration-core:3026/api/v1/sync-jobs/sync%2Fid/events"
        );
    }
}

pub(super) async fn list_sync_jobs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/api/v1/sync-jobs", state.integration_core_url);
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(super) async fn get_sync_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/sync-jobs/{}",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(super) async fn sync_job_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let url = sync_job_events_url(&state.integration_core_url, &id);
    proxy_sse_for_user(&state, &user, &headers, Method::GET, &url).await
}
