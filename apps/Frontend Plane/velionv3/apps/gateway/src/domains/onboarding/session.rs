use axum::{
    routing::{get, post, put},
    Router,
};

use crate::{
    config::AppState,
    onboarding::{
        session::{
            complete_onboarding, get_onboarding_state, onboarding_status, put_onboarding_state,
            session_bootstrap,
        },
        theme::update_brand_theme,
    },
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/session/bootstrap", get(session_bootstrap))
        .route("/api/v1/onboarding/status", get(onboarding_status))
        .route("/api/v1/onboarding/state", get(get_onboarding_state))
        .route("/api/v1/onboarding/state", put(put_onboarding_state))
        .route("/api/v1/onboarding/theme", put(update_brand_theme))
        .route("/api/v1/onboarding/complete", post(complete_onboarding))
}
