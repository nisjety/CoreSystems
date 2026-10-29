# Phase 6 — Client Surfaces & Shells

This directory documents **operator-facing client surfaces** beyond the bare HTTP/gRPC
APIs exposed by `capability-core`, `model-gateway`, `inference-core`, and
`execution-core`. These shells are treated as **first-class product features**, each
with an explicit backend owner inside the Model Plane.

## Scope

Phase 6 covers the delivery surfaces through which humans and tools interact with
the Model Plane. It does **not** add new backend capabilities; instead it fixes the
contract between shells and the existing control plane (`capability-core` for
catalog / policy / scheduling / metadata) and data planes (`inference-core`,
`execution-core`, `model-gateway`).

## Surfaces

| Surface | Document | Primary backend owner |
|---------|----------|-----------------------|
| CLI / TUI / Web shell | [`cli-tui-web-shell.md`](./cli-tui-web-shell.md) | `model-gateway` (ingress) + `capability-core` (catalog/policy) |
| IDE bridge | [`ide-bridge.md`](./ide-bridge.md) | `model-gateway` + `execution-core` |
| Remote session | [`remote-session.md`](./remote-session.md) | `execution-core` (session lifecycle) |
| Voice | [`voice.md`](./voice.md) | `inference-core` (STT/TTS routing) |
| Channel / gateway | [`channel-gateway.md`](./channel-gateway.md) | `model-gateway` |
| Session transfer & remote control | [`session-transfer-remote-control.md`](./session-transfer-remote-control.md) | `execution-core` + `capability-core` |

## Reference inputs

Prior art consulted when drafting these surfaces:

- `claude-code-fork` — terminal-first agent shell, REPL, dialog launchers.
- `openai/codex` — CLI + IDE bridge patterns.
- `openclaw` — remote session / control semantics.
- `hermes-agent` — channel gateway + voice integration.

These are **references only**. No code is copied; the contracts described here are
owned by the Model Plane.

## Acceptance

Phase 6 is complete when every surface above:

1. Has a documented transport and authentication story.
2. Names an explicit backend owner (one of `model-gateway`, `inference-core`,
   `execution-core`, `capability-core`).
3. Enumerates the `capability-core` responsibilities it depends on
   (catalog lookup, policy evaluation, scheduling hints, metadata read).
4. Does **not** require changes to the frozen Phase 0 specifications.
