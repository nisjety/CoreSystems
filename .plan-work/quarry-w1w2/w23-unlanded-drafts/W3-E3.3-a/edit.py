import pathlib
import sys

BASE = pathlib.Path(sys.argv[1])
NEW = BASE / "new"


def read(path):
    return path.read_bytes().decode("utf-8")


def write(path, text):
    path.write_bytes(text.encode("utf-8"))


def once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new)


def crlf(text):
    assert "\r" not in text
    return text.replace("\n", "\r\n")


# 1. browser-session.ts (LF file)
path = NEW / "browser-session.ts"
text = read(path)
text = once(
    text,
    """  return {
    capabilities: session?.capabilities ?? ['annotate', 'dom_select', 'knowledge_ingest'],
    commentAnchors: [
      {
        id: 'comment-1',
        label: '1',
        x: 0.07,
        y: 0.18,
      },
    ],
    consoleEntries: observation?.console_summary ?? [],
""",
    """  return {
    // No session: the gateway advertised no capability, so the fallback claims none.
    capabilities: session?.capabilities ?? [],
    // No placeholder anchor: the annotate capability is not built.
    commentAnchors: [],
    consoleEntries: observation?.console_summary ?? [],
""",
    "browser-session default",
)
write(path, text)

# 2. BrowserChrome.tsx (LF file): remove the placeholder anchor render
path = NEW / "BrowserChrome.tsx"
text = read(path)
text = once(
    text,
    """            <For each={session().commentAnchors}>
              {(anchor) => (
                <span
                  class="knowledge-browser-comment-anchor"
                  style={{
                    left: `${anchor.x * 100}%`,
                    top: `${anchor.y * 100}%`,
                  }}
                  aria-label={`Kommentar ${anchor.label}`}
                >
                  {anchor.label}
                </span>
              )}
            </For>
""",
    "",
    "BrowserChrome anchor render",
)
write(path, text)

# 3. browser.rs (CRLF file)
path = NEW / "browser.rs"
text = read(path)

# 3a. test-only fixture list (browser_response, cfg(test)): drop annotate and inspect_dom
text = once(
    text,
    crlf('"select", "inspect_dom", "control_state",'),
    crlf('"select", "control_state",'),
    "fixture inspect_dom",
)
text = once(
    text,
    crlf('"visual_change", "annotate", "persistent_profile", "evidence"]'),
    crlf('"visual_change", "persistent_profile", "evidence"]'),
    "fixture annotate",
)

# 3b. production list (browser_session_capabilities)
text = once(
    text,
    crlf(
        """/// The capabilities the SPA may act on for a session. The three WS8.D additions are read
/// from Quarry's projection, so none is advertised until Quarry reports it: `human_browsing`
/// needs `grantKind == "human_browse"`, `tab_control` the boolean `tabControl`, and `attach`
/// the boolean `attachAvailable` (false until WS8.F).
fn browser_session_capabilities(projection: &Value) -> Vec<&'static str> {
    let flag = |key: &str| projection.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mut capabilities = vec![
"""
    ),
    crlf(
        """/// The capabilities the SPA may act on for a session. Each names an action or surface that Quarry
/// honours for it. The three WS8.D additions are read from Quarry's projection, so none is
/// advertised until Quarry reports it: `human_browsing` needs `grantKind == "human_browse"`,
/// `tab_control` the boolean `tabControl`, and `attach` the boolean `attachAvailable` (false until
/// WS8.F). Quarry refuses a persistent profile and an attach for a ZDR run (`zdr_matrix` rows
/// `agent_persistent_profile` and `agent_attach`), so `persistent_profile` and `attach` are offered
/// only when the projection says `zdr: false`.
fn browser_session_capabilities(projection: &Value) -> Vec<&'static str> {
    let flag = |key: &str| projection.get(key).and_then(Value::as_bool).unwrap_or(false);
    let zdr_off = projection.get("zdr").and_then(Value::as_bool) == Some(false);
    let mut capabilities = vec![
"""
    ),
    "production doc and header",
)

text = once(
    text,
    crlf('        "select",\n        "inspect_dom",\n        "control_state",\n'),
    crlf('        "select",\n        "control_state",\n'),
    "production inspect_dom",
)

text = once(
    text,
    crlf(
        """        "visual_change",
        "annotate",
        "persistent_profile",
        "evidence",
    ];
    if projection.get("grantKind").and_then(Value::as_str) == Some("human_browse") {
        capabilities.push("human_browsing");
    }
    if flag("tabControl") {
        capabilities.push("tab_control");
    }
    if flag("attachAvailable") {
        capabilities.push("attach");
    }
    capabilities
}
"""
    ),
    crlf(
        """        "visual_change",
        "evidence",
    ];
    if zdr_off {
        capabilities.push("persistent_profile");
    }
    if projection.get("grantKind").and_then(Value::as_str) == Some("human_browse") {
        capabilities.push("human_browsing");
    }
    if flag("tabControl") {
        capabilities.push("tab_control");
    }
    if zdr_off && flag("attachAvailable") {
        capabilities.push("attach");
    }
    capabilities
}
"""
    ),
    "production tail",
)

# 3c. regression test in the same test module, after the human-capabilities test
text = once(
    text,
    crlf(
        """        assert!(!caps.contains(&"attach"));
    }
}
"""
    ),
    crlf(
        """        assert!(!caps.contains(&"attach"));
    }

    #[test]
    fn advertised_capabilities_name_only_what_quarry_honours() {
        let open = browser_session_capabilities(&json!({
            "runId": "run-1",
            "zdr": false,
            "grantKind": "human_browse",
            "tabControl": true,
            "attachAvailable": true
        }));
        assert!(!open.contains(&"annotate"), "{open:?}");
        assert!(!open.contains(&"inspect_dom"), "{open:?}");
        assert!(open.contains(&"persistent_profile"), "{open:?}");
        assert!(open.contains(&"human_browsing"), "{open:?}");
        assert!(open.contains(&"tab_control"), "{open:?}");
        assert!(open.contains(&"attach"), "{open:?}");

        let zdr = browser_session_capabilities(&json!({
            "runId": "run-1",
            "zdr": true,
            "attachAvailable": true
        }));
        assert!(!zdr.contains(&"persistent_profile"), "{zdr:?}");
        assert!(!zdr.contains(&"attach"), "{zdr:?}");
        assert!(zdr.contains(&"navigate"), "{zdr:?}");

        let unstated = browser_session_capabilities(&json!({ "runId": "run-1" }));
        assert!(!unstated.contains(&"persistent_profile"), "{unstated:?}");
    }
}
"""
    ),
    "regression test",
)
write(path, text)
print("ok")
