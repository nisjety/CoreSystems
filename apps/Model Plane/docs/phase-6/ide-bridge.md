# IDE Bridge

## Product role

An in-editor client (VS Code, JetBrains, Neovim) that talks to the same Model
Plane as the CLI, but enriches requests with editor context (open files,
selection, diagnostics) and applies responses as edits/patches.

## Transport

- **Local**: editor extension ↔ local bridge daemon via Unix domain socket or
  loopback WebSocket.
- **Remote**: bridge daemon ↔ `model-gateway` via HTTPS + WS (same contract as
  the CLI shell).
- **Auth**: user bearer token stored by the OS keychain; forwarded by the
  daemon. Editor never sees the raw token.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Local editor context capture | IDE extension (client-side) |
| Patch apply / diff preview | IDE extension (client-side) |
| Request ingress | `model-gateway` |
| Tool execution (fs, shell, lsp) | `execution-core` |
| Catalog / policy / quota | `capability-core` |

## `capability-core` responsibilities

- **Catalog**: filter models by `capability=code_edit` and by context-window
  requirement.
- **Policy**: per-repo overrides (e.g. forbid external providers for
  `org=triodelab, repo=private`).
- **Scheduling hints**: prefer low-latency region for interactive edits.
- **Metadata**: tool-use contracts the IDE should advertise.

## Reference inputs

- `openai/codex` — IDE session lifecycle, patch protocol.
- `claude-code-fork` — edit/confirm loop, dialog launchers.

## Out of scope

- Editor-specific UI.
- Language-server logic (LSP lives inside each editor; the bridge only forwards
  structured results).
