import os

os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces")
p = "mod.rs"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")


def sub(old, new, count=1):
    global b
    assert b.count(old) == count, (b.count(old), old[:90])
    b = b.replace(old, new)


# ---- instructions fixture: one probe that also reports what the upstreams were asked ----------
sub(
    """    async fn space_instructions_response(
        role: &str,
        method: &str,
        path: &str,
        write_body: Option<Value>,
    ) -> (u16, Value) {
        let auth = MockServer::start().await;""",
    """    fn org_zdr_posture(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(
            json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
        )
    }

    async fn space_instructions_response(
        role: &str,
        method: &str,
        path: &str,
        write_body: Option<Value>,
    ) -> (u16, Value) {
        let (status, body, _) =
            space_instructions_probe(role, method, path, write_body, org_zdr_posture(false)).await;
        (status, body)
    }

    /// Like `space_instructions_response`, with org-core's answer to the ZDR posture read, and the
    /// `METHOD path` of every request the Control and Application mocks were asked.
    async fn space_instructions_probe(
        role: &str,
        method: &str,
        path: &str,
        write_body: Option<Value>,
        org_core: ResponseTemplate,
    ) -> (u16, Value, Vec<String>) {
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_core)
            .mount(&org)
            .await;
        let auth = MockServer::start().await;""",
)
sub(
    """        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();

        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", "better-auth.session_token=session-1");""",
    """        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org.uri();

        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", "better-auth.session_token=session-1");""",
)
sub(
    """            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    #[tokio::test]
    async fn a_viewer_can_read_space_instructions() {""",
    """            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        let mut asked = Vec::new();
        for server in [&user_core, &application] {
            for received in server.received_requests().await.unwrap_or_default() {
                asked.push(format!("{} {}", received.method, received.url.path()));
            }
        }
        (status, parsed, asked)
    }

    #[tokio::test]
    async fn a_zdr_org_cannot_write_space_instructions_and_nothing_upstream_is_asked() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, asked) = space_instructions_probe(
            "owner",
            "PATCH",
            "/api/v1/spaces/space-1/instructions",
            Some(json!({"instructions": "New rule."})),
            org_zdr_posture(true),
        )
        .await;
        assert_eq!(status, 412, "{body}");
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(
            !asked.iter().any(|line| line.contains("membership") || line.contains("/api/mutation")),
            "a ZDR refusal comes before the Control role read and the Convex write: {asked:?}"
        );
    }

    #[tokio::test]
    async fn space_instructions_say_so_when_org_core_cannot_answer_the_zdr_posture() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, asked) = space_instructions_probe(
            "owner",
            "PATCH",
            "/api/v1/spaces/space-1/instructions",
            Some(json!({"instructions": "New rule."})),
            ResponseTemplate::new(500),
        )
        .await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "zdr_decision_unavailable");
        assert_eq!(body["error"]["retryable"], true);
        assert!(!asked.iter().any(|line| line.contains("/api/mutation")), "{asked:?}");
    }

    #[tokio::test]
    async fn a_zdr_org_can_still_read_space_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body, _) = space_instructions_probe(
            "viewer",
            "GET",
            "/api/v1/spaces/space-1/instructions",
            None,
            org_zdr_posture(true),
        )
        .await;
        assert_eq!(status, 200, "{body}");
    }

    #[tokio::test]
    async fn a_viewer_can_read_space_instructions() {""",
)
b2 = b
if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("instructions tests added")
