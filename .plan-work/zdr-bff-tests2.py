import os

os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces")
p = "mod.rs"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")


def sub(old, new):
    global b
    assert b.count(old) == 1, (b.count(old), old[:90])
    b = b.replace(old, new)


# (a) the ZDR check moves behind the cheap role and org checks, right before the Control call
sub(
    """    // Creating a grant is a durable write (plan 1.3 #8) and refuses for a ZDR org; revoking one is
    // allowed under every posture, like every other delete.
    if operation == "create" {
        if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, None, None).await {
            return refusal.into_response();
        }
    }
""",
    "",
)
sub(
    """                "An active organization membership is required to manage agent action grants.",
            )),
        )
            .into_response();
    }
    let actor = ActionActor {""",
    """                "An active organization membership is required to manage agent action grants.",
            )),
        )
            .into_response();
    }
    // Creating a grant is a durable write (plan 1.3 #8) and refuses for a ZDR org, before any Control
    // call; revoking one is allowed under every posture, like every other delete.
    if operation == "create" {
        if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, None, None).await {
            return refusal.into_response();
        }
    }
    let actor = ActionActor {""",
)

# (b) shared helpers: every control mock also answers the org posture (not ZDR), and state_for points at it
sub(
    """        state.user_core_url = user_core.uri();
        state.space_features.denials_typed = denials_typed;
        state
    }
""",
    """        state.user_core_url = user_core.uri();
        // `refusing_control` also answers org-core's posture read on the same server (not ZDR).
        state.org_core_url = user_core.uri();
        state.space_features.denials_typed = denials_typed;
        state
    }
""",
)
sub(
    """            .and(wm_path(path))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&user_core)
            .await;
        user_core
    }
""",
    """            .and(wm_path(path))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_zdr_posture(false))
            .mount(&user_core)
            .await;
        user_core
    }
""",
)

# (c) the end-to-end grant test needs an org-core that says "not ZDR"
sub(
    """        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.conversation_core_url = conversation.uri();
""",
    """        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_zdr_posture(false))
            .mount(&org_core)
            .await;

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.conversation_core_url = conversation.uri();
        state.org_core_url = org_core.uri();
""",
)

# (d) new tests
sub(
    """    #[tokio::test]
    async fn owner_grant_refusal_is_typed_only_with_the_flag_on() {""",
    """    #[tokio::test]
    async fn a_zdr_org_cannot_create_an_agent_action_grant_but_can_revoke_one() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let owner = || {
            let mut owner = user();
            owner.auth_role = Some("owner".to_owned());
            owner.authorized_membership = Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "owner".to_owned(),
            });
            owner
        };
        let control =
            refusing_control("/api/v1/internal/spaces/owner-grant-decision", 403, typed_denial()).await;
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_zdr_posture(true))
            .mount(&org)
            .await;
        let mut state = state_for(&control, false);
        state.org_core_url = org.uri();

        let created = forward_owner_grant_mutation(
            &state,
            &owner(),
            "space-1",
            "conversation-1",
            "create",
            None,
            OwnerGrantMutationRequest { idempotency_key: "grant-1".to_owned() },
        )
        .await;
        assert_eq!(created.status(), axum::http::StatusCode::PRECONDITION_FAILED);
        let body: Value =
            serde_json::from_slice(&created.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        let decision_calls = |requests: Vec<wiremock::Request>| {
            requests
                .iter()
                .filter(|request| request.url.path() == "/api/v1/internal/spaces/owner-grant-decision")
                .count()
        };
        assert_eq!(
            decision_calls(control.received_requests().await.unwrap()),
            0,
            "a ZDR create is refused before any Control decision is asked for"
        );

        // A revoke is allowed under every posture: it reaches Control (which refuses it here, 403).
        let revoked = forward_owner_grant_mutation(
            &state,
            &owner(),
            "space-1",
            "conversation-1",
            "revoke",
            Some("grant-1"),
            OwnerGrantMutationRequest { idempotency_key: "revoke-1".to_owned() },
        )
        .await;
        assert_eq!(revoked.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(decision_calls(control.received_requests().await.unwrap()), 1);
    }

    #[tokio::test]
    async fn owner_grant_refusal_is_typed_only_with_the_flag_on() {""",
)

if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("grant tests edited")
