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

It exercises the real library: the real connect sequence, the real crypto, the real canvas renderer. It has **not** yet been run against a live `rustdesk-server` plus a native host — so a failure here is as likely to be a protocol-interop gap as a bug in the harness. `docs/architecture.md` in remote-core lists, in priority order, exactly which unknowns a first live run should settle.
