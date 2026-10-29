use serde_json::Value;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xero-mcp"));
    command.env_clear();
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

#[test]
fn manifest_is_available_without_credentials_and_env_errors_are_redacted() {
    let output = binary().arg("--print-manifest").output().unwrap();
    assert!(output.status.success());
    let manifest: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest["catalog_operation_count"], 186);
    assert_eq!(manifest["operation_count"], 184);
    assert_eq!(manifest["server_name"], "xero-mcp");
    assert_eq!(manifest["source_writes"], false);
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("bad.env");
    std::fs::write(&invalid, "PRIVATE_SECRET='unterminated").unwrap();
    for path in [invalid, directory.path().join("absent.env")] {
        let output = binary().arg("--env-file").arg(path).output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("MCP environment file is missing or invalid"));
        assert!(!error.contains("PRIVATE_SECRET"));
    }
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn http_process_starts_with_private_configuration_and_protects_capabilities() {
    let directory = tempfile::tempdir().unwrap();
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserve.local_addr().unwrap();
    drop(reserve);
    let token = "test-only-credential-not-for-production";
    let mut child = Child(
        binary()
            .env("XERO_AUTH_MODE", "pkce")
            .env("XERO_PKCE_CLIENT_ID", "fixture-client")
            .env("XERO_CONNECTION_STORE_BACKEND", "file")
            .env(
                "XERO_CONNECTION_STORE_PATH",
                directory.path().join("connections.json"),
            )
            .env("XERO_MCP_TENANTS", "5f3c9a2e-0000-0000-0000-000000000000")
            .env("XERO_MCP_TOKEN", token)
            .env("XERO_MCP_TOOL_MODE", "compact")
            .env("XERO_MCP_BIND", address.to_string())
            .args(["--transport", "http"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let url = format!("http://{address}");
    let started = Instant::now();
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "MCP exited before listening"
        );
        if let Ok(response) = client.get(format!("{url}/healthz")).send().await {
            assert!(response.status().is_success());
            let health: Value = response.json().await.unwrap();
            assert_eq!(health["service"], "xero-mcp");
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(60));
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let unauthenticated = client
        .get(format!("{url}/capabilities"))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), 401);
    assert_eq!(
        unauthenticated.headers()["www-authenticate"],
        "Bearer realm=\"xero-mcp\""
    );
    let manifest: Value = client
        .get(format!("{url}/capabilities"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(manifest["catalog_operation_count"], 186);
    assert_eq!(manifest["operation_count"], 184);
    assert_eq!(manifest["server_name"], "xero-mcp");
}

#[tokio::test]
async fn stdio_process_keeps_stdout_protocol_clean_and_exits_after_host_disconnect() {
    use rmcp::{model::ClientConfig, ServiceExt};
    let directory = tempfile::tempdir().unwrap();
    let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_xero-mcp"))
        .env_clear()
        .env(
            "LLVM_PROFILE_FILE",
            std::env::var("LLVM_PROFILE_FILE").unwrap_or_else(|_| {
                directory
                    .path()
                    .join("%p-%m.profraw")
                    .to_string_lossy()
                    .into_owned()
            }),
        )
        .env("XERO_AUTH_MODE", "pkce")
        .env("XERO_PKCE_CLIENT_ID", "fixture")
        .env("XERO_CONNECTION_STORE_BACKEND", "file")
        .env(
            "XERO_CONNECTION_STORE_PATH",
            directory.path().join("connections.json"),
        )
        .env("XERO_MCP_TENANTS", "5f3c9a2e-0000-0000-0000-000000000000")
        .env("XERO_MCP_TOOL_MODE", "compact")
        .args(["--transport", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let transport = (
        process.stdout.take().unwrap(),
        process.stdin.take().unwrap(),
    );
    let client = tokio::time::timeout(
        Duration::from_secs(60),
        ClientConfig::default().serve(transport),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 14);
    client.cancel().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(5), process.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}
