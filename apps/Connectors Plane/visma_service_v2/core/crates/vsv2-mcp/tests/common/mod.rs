//! Minimal MCP stdio client for black-box tests: spawns the real binary and speaks JSON-RPC.
#![allow(dead_code)]

use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

/// What the test client does when the server asks the user a question (MCP elicitation).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Elicit {
    /// The client does not declare the capability at all.
    Unsupported,
    /// The simulated user says yes.
    Accept,
    /// The simulated user says no.
    Decline,
}

pub struct McpClient {
    /// Every approval question the server asked, verbatim.
    pub elicitations: Vec<String>,
    elicit: Elicit,
    _cwd: Option<tempfile::TempDir>,
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    next_id: u64,
}

pub fn repo_root() -> String {
    format!("{}/../../..", env!("CARGO_MANIFEST_DIR"))
}

pub fn repo_endpoint_dir() -> String {
    format!("{}/config/endpoints", repo_root())
}

impl McpClient {
    /// Hermetic: clean env and an empty working directory, so neither the host environment nor a
    /// `.env` found by walking up from the repo can enable live tools. No elicitation support.
    pub async fn spawn(env: &[(&str, &str)]) -> Self {
        Self::spawn_with(env, Elicit::Unsupported).await
    }

    /// Hermetic, with a simulated user for approval prompts.
    pub async fn spawn_with(env: &[(&str, &str)], elicit: Elicit) -> Self {
        let cwd = tempfile::tempdir().unwrap();
        Self::start(env, cwd.path().to_path_buf(), Some(cwd), elicit).await
    }

    /// Live: runs from the repository root so `.env` (real credentials) is found. Read-only tests only.
    pub async fn spawn_live(env: &[(&str, &str)]) -> Self {
        Self::start(env, repo_root().into(), None, Elicit::Unsupported).await
    }

    async fn start(
        env: &[(&str, &str)],
        cwd: std::path::PathBuf,
        keep: Option<tempfile::TempDir>,
        elicit: Elicit,
    ) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vsv2-mcp"));
        cmd.current_dir(cwd);
        cmd.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            // Never pick up a real sign-in of whoever runs the tests; a test that wants one passes its own store.
            .env(
                "VSV2_USER_STORE_DIR",
                std::env::temp_dir().join("vsv2-mcp-tests-no-sign-in"),
            )
            .env("VSV2_ENDPOINT_CONFIG_DIR", repo_endpoint_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn vsv2-mcp");
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut c = Self {
            elicitations: vec![],
            elicit,
            _cwd: keep,
            child,
            stdin,
            out,
            next_id: 1,
        };
        c.initialize().await;
        c
    }

    async fn send(&mut self, v: Value) {
        let mut line = serde_json::to_string(&v).unwrap();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn read_response(&mut self, id: u64) -> Value {
        loop {
            let mut line = String::new();
            let n = timeout(Duration::from_secs(60), self.out.read_line(&mut line))
                .await
                .expect("server answered in time")
                .unwrap();
            assert!(n > 0, "server closed stdout");
            let v: Value =
                serde_json::from_str(line.trim()).expect("stdout must only carry JSON-RPC");
            // A request *from the server* (it has a method and an id): answer it as the user would.
            if v.get("method").is_some() && v.get("id").is_some() {
                self.answer_server_request(&v).await;
                continue;
            }
            if v.get("id") == Some(&json!(id)) {
                return v;
            }
        }
    }

    async fn answer_server_request(&mut self, req: &Value) {
        let id = req["id"].clone();
        let reply = if req["method"] == "elicitation/create" {
            self.elicitations.push(
                req["params"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            );
            match self.elicit {
                Elicit::Accept => json!({"jsonrpc":"2.0","id":id,
                    "result":{"action":"accept","content":{"approve":true}}}),
                _ => json!({"jsonrpc":"2.0","id":id,"result":{"action":"decline"}}),
            }
        } else {
            json!({"jsonrpc":"2.0","id":id,
                "error":{"code":-32601,"message":"not supported by the test client"}})
        };
        self.send(reply).await;
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        self.read_response(id).await
    }

    async fn initialize(&mut self) {
        let capabilities = if self.elicit == Elicit::Unsupported {
            json!({})
        } else {
            json!({"elicitation": {"form": {}}})
        };
        let r = self
            .request(
                "initialize",
                json!({"protocolVersion":"2025-06-18","capabilities":capabilities,
                       "clientInfo":{"name":"vsv2-test","version":"0"}}),
            )
            .await;
        assert!(r.get("result").is_some(), "initialize failed: {r}");
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
    }

    pub async fn tools(&mut self) -> Vec<Value> {
        let r = self.request("tools/list", json!({})).await;
        r["result"]["tools"].as_array().cloned().unwrap_or_default()
    }

    /// Returns the raw JSON-RPC response (`result` or `error`).
    pub async fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": args}))
            .await
    }

    pub fn server_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}
