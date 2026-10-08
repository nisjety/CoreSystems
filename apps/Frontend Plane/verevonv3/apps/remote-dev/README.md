# remote-dev

Development-only harness for `@verevon/remote-core`. **Not part of the Verevon product UI** — it exists so the library can be driven by hand against a real Support Plane. All visible UI text is Norwegian, matching the product spec's requirement for the demo surface.

## Setup

```bash
cp .env.example .env.local     # then fill in the values below
pnpm --filter remote-dev dev   # http://localhost:5183
```

`@verevon/remote-core` is consumed from its build output, so build it first (or after any library change):

```bash
pnpm --filter @verevon/remote-core build
```

## Configuration

| Variable | Purpose |
|---|---|
| `VITE_REMOTE_RENDEZVOUS_URL` | hbbs WebSocket endpoint — `ws://localhost:21118` locally, `wss://…` through Caddy |
| `VITE_REMOTE_RELAY_URL` | hbbr WebSocket endpoint. Optional; overrides whatever address the server advertises (needed when hbbr sits behind its own TLS proxy) |
| `VITE_REMOTE_SERVER_PUBLIC_KEY` | Base64 Ed25519 public key of the rendezvous server. **Without it the peer cannot be authenticated**, and this harness deliberately opts into `allowUnverifiedPeer` so a keyless local test rig still works. Never do that outside local development — see `docs/security.md` in remote-core. |

## What this can and cannot tell you

It exercises the real library: the real connect sequence, the real crypto, the real canvas renderer, and (since 2026-09-11) real pointer/keyboard forwarding — the canvas mirrors `RemoteSupportPage.tsx`'s exact `canPoint`/`canType`-gated handlers, and granted permissions show in the status bar. It has been run successfully against a real `rustdesk-server` plus a stock RustDesk client (see `docs/architecture.md`'s Status section) for the video/connect path; pointer/keyboard forwarding has the wiring but has not itself been exercised against a real host yet. `docs/architecture.md` lists, in priority order, exactly which unknowns remain.

A test rig without a separate Verevon Agent machine is a loopback: the "remote" host is the same machine running the browser controller. Pointer/keyboard testing this way drives the *operator's own* live desktop — be as careful with it as with any other live, unattended system before sending it clicks or keystrokes.
