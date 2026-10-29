//! `vsv2-mcp login | logout | whoami` through the real binary against a mock Visma Connect: the commands, the
//! terminal handling, `.env`-style configuration from the environment, and what ends up on disk.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use mockito::Matcher;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use vsv2_signin::{Connection, Session, Store};

const READ: &str = "vismanet_erp_interactive_api:read";

fn command(args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vsv2-mcp"));
    cmd.args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd
}

async fn run(args: &[&str], env: &[(&str, &str)]) -> (bool, String, String) {
    let mut cmd = command(args, env);
    cmd.stdin(Stdio::null());
    let out = timeout(Duration::from_secs(60), cmd.output())
        .await
        .expect("finished in time")
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn connection(tenant: &str, name: &str) -> Connection {
    let mut c = Connection::new(
        tenant.into(),
        "user-1".into(),
        vec![READ.into()],
        format!("refresh-{tenant}"),
        format!("access-{tenant}"),
        Utc::now() + chrono::Duration::hours(1),
    );
    c.name = Some(name.into());
    c
}

fn signed_in_store(dir: &std::path::Path) -> Store {
    let store = Store::new(dir.join("user"));
    store
        .save_session(&Session::new(
            "user-1".into(),
            Some("ima@example.com".into()),
            "web-app".into(),
        ))
        .unwrap();
    store
        .save_connection(&connection("t-1", "Aquatiq AS"))
        .unwrap();
    store
        .save_connection(&connection("t-2", "Aqua Pharma Inc"))
        .unwrap();
    store
}

#[tokio::test]
async fn whoami_shows_the_person_the_companies_and_their_scopes_from_disk_alone() {
    let dir = tempfile::tempdir().unwrap();
    signed_in_store(dir.path());
    let store_dir = dir.path().join("user").display().to_string();
    let (ok, out, err) = run(&["whoami"], &[("VSV2_USER_STORE_DIR", &store_dir)]).await;
    assert!(ok, "{err}");
    assert!(
        out.contains("ima@example.com") && out.contains("user-1"),
        "{out}"
    );
    assert!(
        out.contains("Aquatiq AS (t-1)") && out.contains("Aqua Pharma Inc (t-2)"),
        "{out}"
    );
    assert!(
        out.contains(READ) && out.contains("access token valid"),
        "{out}"
    );
    assert!(
        !out.contains("refresh-t-1") && !out.contains("access-t-1"),
        "no token is ever printed: {out}"
    );

    let empty = dir.path().join("nobody").display().to_string();
    let (ok, out, _) = run(&["whoami"], &[("VSV2_USER_STORE_DIR", &empty)]).await;
    assert!(ok && out.contains("Nobody is signed in"), "{out}");
}

#[tokio::test]
async fn whoami_check_asks_visma_whether_each_sign_in_is_still_accepted() {
    let mut visma = mockito::Server::new_async().await;
    // t-1's refresh token is accepted, t-2's has been revoked.
    let accepted = visma
        .mock("POST", "/token")
        .match_body(Matcher::UrlEncoded(
            "refresh_token".into(),
            "refresh-t-1".into(),
        ))
        .with_body(r#"{"access_token":"renewed","expires_in":3600}"#)
        .expect(1)
        .create_async()
        .await;
    let revoked = visma
        .mock("POST", "/token")
        .match_body(Matcher::UrlEncoded(
            "refresh_token".into(),
            "refresh-t-2".into(),
        ))
        .with_status(400)
        .with_body(r#"{"error":"invalid_grant"}"#)
        .expect(1)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    signed_in_store(dir.path());
    let env = [
        (
            "VSV2_USER_STORE_DIR",
            dir.path().join("user").display().to_string(),
        ),
        ("VISMA_USER_CLIENT_ID", "web-app".to_string()),
        ("VISMA_USER_CLIENT_SECRET", "s3cret".to_string()),
        ("VISMA_USER_TOKEN_URL", format!("{}/token", visma.url())),
    ];
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (ok, out, err) = run(&["whoami", "--check"], &pairs).await;
    assert!(ok, "{err}");
    assert!(
        out.contains("Aquatiq AS (t-1)") && out.contains("Visma accepts it"),
        "{out}"
    );
    assert!(
        out.contains("Aqua Pharma Inc (t-2) | REJECTED") && out.contains("vsv2-mcp login"),
        "{out}"
    );
    accepted.assert_async().await;
    revoked.assert_async().await;
}

#[tokio::test]
async fn logout_revokes_every_refresh_token_at_visma_and_forgets_the_sign_in() {
    let mut visma = mockito::Server::new_async().await;
    let revoke = visma
        .mock("POST", "/revoke")
        .match_body(Matcher::UrlEncoded(
            "token_type_hint".into(),
            "refresh_token".into(),
        ))
        .with_status(200)
        .expect(2)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let store = signed_in_store(dir.path());
    let env = [
        (
            "VSV2_USER_STORE_DIR",
            dir.path().join("user").display().to_string(),
        ),
        ("VISMA_USER_CLIENT_ID", "web-app".to_string()),
        ("VISMA_USER_CLIENT_SECRET", "s3cret".to_string()),
        (
            "VISMA_USER_REVOCATION_URL",
            format!("{}/revoke", visma.url()),
        ),
    ];
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (ok, out, err) = run(&["logout"], &pairs).await;
    assert!(ok, "{err}");
    assert!(
        out.contains("Forgot 2 companies (2 revoked at Visma, 0 could not be)"),
        "{out}"
    );
    assert!(out.contains("service credentials again"), "{out}");
    revoke.assert_async().await;
    assert!(store.session().unwrap().is_none() && store.connections().unwrap().is_empty());
}

#[tokio::test]
async fn a_mistyped_flag_or_missing_application_is_explained_and_nothing_starts() {
    let (ok, _, err) = run(&["login", "--wirte"], &[]).await;
    assert!(
        !ok && err.contains("unknown argument") && err.contains("usage:"),
        "{err}"
    );
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("user").display().to_string();
    let (ok, _, err) = run(&["login"], &[("VSV2_USER_STORE_DIR", &store)]).await;
    assert!(
        !ok && err.contains("VISMA_USER_CLIENT_ID") && err.contains("Developer Portal"),
        "{err}"
    );
}

fn jwt(payload: &serde_json::Value) -> String {
    format!(
        "{}.{}.signature",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}

/// The `code|tenant|person` the paste test hands over becomes tokens for that company and person.
async fn mock_connect(server: &mut mockito::ServerGuard) {
    server
        .mock("POST", "/token")
        .with_body_from_request(|request| {
            let form: HashMap<String, String> = url::form_urlencoded::parse(request.body().unwrap()).into_owned().collect();
            let code = form.get("code").cloned().unwrap_or_default();
            let mut parts = code.split('|').skip(1);
            let (tenant, sub) = (parts.next().unwrap_or("t-?"), parts.next().unwrap_or("user-1"));
            json!({
                "access_token": jwt(&json!({"sub": sub, "tenant_id": tenant, "exp": Utc::now().timestamp() + 3600})),
                "refresh_token": format!("refresh-{tenant}"),
                "expires_in": 3600,
                "scope": format!("openid offline_access {READ}")
            })
            .to_string()
            .into_bytes()
        })
        .expect_at_least(1)
        .create_async()
        .await;
    server
        .mock("GET", "/v1/organization")
        .match_query(Matcher::Any)
        .with_body_from_request(|request| {
            let tenant = request
                .header("ipp-company-id")
                .first()
                .map(|v| v.to_str().unwrap().to_string())
                .unwrap_or_default();
            json!([{"name": format!("Company {tenant}")}])
                .to_string()
                .into_bytes()
        })
        .expect_at_least(0)
        .create_async()
        .await;
}

#[tokio::test]
async fn login_with_pasted_addresses_connects_company_after_company_and_stops_when_the_input_ends()
{
    let mut visma = mockito::Server::new_async().await;
    mock_connect(&mut visma).await;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path().join("user"));
    let env = [
        (
            "VSV2_USER_STORE_DIR",
            dir.path().join("user").display().to_string(),
        ),
        ("VISMA_USER_CLIENT_ID", "web-app".to_string()),
        ("VISMA_USER_CLIENT_SECRET", "s3cret".to_string()),
        (
            "VISMA_USER_REDIRECT_URI",
            "https://app.example.com/callback".to_string(),
        ),
        ("VISMA_USER_TOKEN_URL", format!("{}/token", visma.url())),
        ("VISMA_ERP_REST_BASE_URL", visma.url()),
    ];
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut child = command(&["login", "--paste"], &pairs)
        .spawn()
        .expect("spawn login");
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut transcript = String::new();

    // Visma asks for one company at a time: answer two consent screens, then end the input.
    for company in ["t-1", "t-2"] {
        let state = loop {
            let line = timeout(Duration::from_secs(30), lines.next_line())
                .await
                .expect("output in time")
                .unwrap()
                .expect("login is still running");
            transcript.push_str(&line);
            transcript.push('\n');
            if let Some(url) = line
                .trim()
                .strip_prefix("https://connect.visma.com/connect/authorize?")
            {
                let query: HashMap<String, String> = url::form_urlencoded::parse(url.as_bytes())
                    .into_owned()
                    .collect();
                assert_eq!(query["code_challenge_method"], "S256");
                assert!(query["scope"].contains("offline_access"), "{url}");
                assert_eq!(query["redirect_uri"], "https://app.example.com/callback");
                break query["state"].clone();
            }
        };
        let answer = format!(
            "https://app.example.com/callback?code={}&state={state}",
            url::form_urlencoded::byte_serialize(format!("code|{company}|user-1").as_bytes())
                .collect::<String>()
        );
        stdin
            .write_all(format!("{answer}\n").as_bytes())
            .await
            .unwrap();
        stdin.flush().await.unwrap();
    }
    drop(stdin);
    while let Some(line) = timeout(Duration::from_secs(30), lines.next_line())
        .await
        .expect("output in time")
        .unwrap()
    {
        transcript.push_str(&line);
        transcript.push('\n');
    }
    let status = timeout(Duration::from_secs(30), child.wait())
        .await
        .expect("exited")
        .unwrap();
    assert!(status.success(), "{transcript}");
    assert!(
        transcript.contains("Connected Company t-1 (t-1)")
            && transcript.contains("Connected Company t-2 (t-2)"),
        "{transcript}"
    );
    assert!(
        transcript.contains("2 companies are connected in total"),
        "{transcript}"
    );
    assert!(
        transcript.contains("stopped at your request"),
        "{transcript}"
    );
    assert!(
        !transcript.contains("s3cret") && !transcript.contains("refresh-t-1"),
        "{transcript}"
    );

    let companies = store.connections().unwrap();
    assert_eq!(companies.len(), 2);
    assert_eq!(companies[0].refresh_token, "refresh-t-1");
    assert_eq!(companies[1].name.as_deref(), Some("Company t-2"));
    assert_eq!(store.session().unwrap().unwrap().sub, "user-1");
}
