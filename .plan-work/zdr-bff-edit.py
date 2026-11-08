import os

GW = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/apps/gateway/src"
os.chdir(GW)


def edit(p, pairs):
    b = open(p, "rb").read().decode("utf-8")
    crlf = "\r\n" in b
    b = b.replace("\r\n", "\n")
    for old, new in pairs:
        assert old in b, (p, old[:80])
        assert b.count(old) == 1, (p, "ambiguous", old[:80])
        b = b.replace(old, new, 1)
    if crlf:
        b = b.replace("\n", "\r\n")
    open(p, "wb").write(b.encode("utf-8"))


# 1. the shared helper
edit("zdr.rs", [(
    'pub(crate) const ZDR_DECISION_UNAVAILABLE: &str = "zdr_decision_unavailable";\n',
    '''pub(crate) const ZDR_DECISION_UNAVAILABLE: &str = "zdr_decision_unavailable";

/// A durable write of stored content is refused for a Zero Data Retention org, typed, before any
/// Control or model call: the guard behind every `DurableWrite` row of the Space surface registry
/// (`space-surface.v1.json`). `Some` is the response to send.
///
/// A request-level flag (the header or a body `zdr`) counts as ZDR. The org posture comes from
/// org-core and fails closed in its own words: an outage is the typed, retryable
/// `zdr_decision_unavailable` (503), never a guess in either direction.
pub(crate) async fn durable_write_refusal(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    body: Option<&Value>,
) -> Option<(axum::http::StatusCode, axum::Json<Value>)> {
    use axum::http::StatusCode;
    let forbidden = || {
        (
            StatusCode::PRECONDITION_FAILED,
            axum::Json(zdr_refusal_body(ZDR_DURABLE_MUTATION_FORBIDDEN)),
        )
    };
    let no_headers = HeaderMap::new();
    if request_zdr(headers.unwrap_or(&no_headers), body) {
        return Some(forbidden());
    }
    match org_zdr_lookup(state, user).await {
        OrgZdrPosture::Enabled => Some(forbidden()),
        OrgZdrPosture::Unknown => Some((
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(zdr_refusal_body(ZDR_DECISION_UNAVAILABLE)),
        )),
        OrgZdrPosture::Disabled => None,
    }
}
''')])

# 2. human posts delegate to it (same behaviour, one implementation)
edit("domains/spaces/human_posts.rs", [(
    '''    let forbidden = || {
        (
            StatusCode::PRECONDITION_FAILED,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DURABLE_MUTATION_FORBIDDEN)),
        )
    };
    if zdr::request_zdr(headers, Some(body)) {
        return Some(forbidden());
    }
    match zdr::org_zdr_lookup(state, user).await {
        OrgZdrPosture::Enabled => Some(forbidden()),
        // An org-core failure is an outage, not a posture: say so, typed, and let the caller retry.
        OrgZdrPosture::Unknown => Some((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DECISION_UNAVAILABLE)),
        )),
        OrgZdrPosture::Disabled => None,
    }
}''',
    '''    zdr::durable_write_refusal(state, user, Some(headers), Some(body)).await
}''')])

# 3. instructions PATCH
edit("domains/spaces/mod.rs", [(
    '''    let role = match resolve_space_role(&state, &user, &org_id, space_ref).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !SPACE_INSTRUCTIONS_WRITE_ROLES.contains(&role.as_str()) {''',
    '''    // Authored instruction text is stored content: refused for a ZDR org before any Control call.
    if let Some(refusal) = crate::zdr::durable_write_refusal(&state, &user, None, None).await {
        return refusal;
    }
    let role = match resolve_space_role(&state, &user, &org_id, space_ref).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !SPACE_INSTRUCTIONS_WRITE_ROLES.contains(&role.as_str()) {'''),
    # 4. grant create
    ('''                "Owner grant operation is invalid.",
            )),
        )
            .into_response();
    }
''',
     '''                "Owner grant operation is invalid.",
            )),
        )
            .into_response();
    }
    // Creating a grant is a durable write (plan 1.3 #8) and refuses for a ZDR org; revoking one is
    // allowed under every posture, like every other delete.
    if operation == "create" {
        if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, None, None).await {
            return refusal.into_response();
        }
    }
''')])
print("edited")
