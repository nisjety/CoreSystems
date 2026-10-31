use axum::{
    extract::State,
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::state::AppState;

pub async fn require_bearer_token(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected_token) = state.bearer_token() else {
        return Ok(next.run(request).await);
    };

    let header_value = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let provided_token = header_value
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !constant_time_eq(provided_token, expected_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut diff = left.len() ^ right.len();

    for index in 0..left.len().max(right.len()) {
        let lhs = left.get(index).copied().unwrap_or_default();
        let rhs = right.get(index).copied().unwrap_or_default();
        diff |= usize::from(lhs ^ rhs);
    }

    diff == 0
}
