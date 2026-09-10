# Verevon Support Plane

**Status:** Runs locally; a full remote-desktop session has been connected end to end through this stack (verified 2026-09-10/11: a real stock RustDesk client registered against local hbbs, and `remote-core` completed rendezvous → relay pairing → peer handshake → login and streamed real decoded video for over an hour) — not yet operated in production, and still no real Verevon Agent or automated integration test
**Last updated:** 2026-09-11
**Product owner:** Support / Remote Assistance
**Runtime position:** Backend infrastructure for `@verevon/remote-core` (`apps/Frontend Plane/verevonv3/packages/remote-core`)

## Purpose

Support Plane hosts the RustDesk-compatible rendezvous and relay servers (`hbbs`/`hbbr`) that a Verevon support agent's browser session and a customer's native Verevon Agent both connect through. It is the "RustDesk hbbs / hbbr" box in `remote-core`'s own architecture diagram — the network hop between `@verevon/remote-core` (browser) and the Verevon Agent (native, runs on the customer's machine).

```text
Verevon SPA (browser, support agent)
        │  wss://support.verevon.com:443  (Caddy — TLS termination)
        ▼
Support Plane
  ├── hbbs   (rendezvous: peer lookup, relay negotiation)
  └── hbbr   (relay: forwards the end-to-end encrypted session once paired)
        │  native TCP/UDP (21115-21117) — no TLS needed at this hop, the
        │  session itself is end-to-end encrypted (see remote-core's
        │  docs/rustdesk-protocol.md, "Encryption / handshake")
        ▼
Verevon Agent (native, on the customer's machine)
```

Support Plane runs the **official, unmodified** `rustdesk-server` (`hbbs`/`hbbr`) binaries as separate network services — it does not fork, patch, or embed any RustDesk source. See `remote-core`'s [`docs/licensing.md`](../Frontend%20Plane/verevonv3/packages/remote-core/docs/licensing.md) for why that specific choice (run-as-a-process, scenario (a)) is the low-friction one under AGPL-3.0, and [`docs/rustdesk-protocol.md`](../Frontend%20Plane/verevonv3/packages/remote-core/docs/rustdesk-protocol.md) for the full protocol research this deployment is based on.

## Why a browser needs a TLS-terminating proxy here

`hbbs`/`hbbr`'s own WebSocket listeners (ports 21118/21119) are **plain `ws://`, never `wss://`** — confirmed directly from the server's source, which explicitly documents that a reverse proxy is expected to sit in front (see `rustdesk-protocol.md`). A page served over `https://` cannot open a plain `ws://` connection (mixed-content blocking), so Support Plane fronts those two ports with Caddy, which obtains and renews TLS certificates automatically and forwards `X-Real-IP`/`X-Forwarded-For` so hbbs/hbbr can still see real client IPs for rate limiting.

**The native ports (21115-21117) are exposed directly, without TLS** — the Verevon Agent (a native process, not a browser) talks raw TCP/UDP to them, and the peer-to-peer session itself is already end-to-end encrypted independently of transport (X25519 + XSalsa20-Poly1305, per `rustdesk-protocol.md`). Only the browser-facing WS ports need the extra TLS hop to satisfy the browser's own mixed-content rules.

## Architectural facts this deployment relies on

All verified in `remote-core`'s `docs/rustdesk-protocol.md` — repeated here because they directly shape this deployment:

- **hbbs/hbbr already support WebSocket natively** — no custom translation layer needed, just a TLS-terminating proxy in front.
- **A browser can only ever be the controlling/connecting side**, never a dialable peer (registration is UDP-only, rejected over TCP/WS). Support Plane's public WS ports are only used by the Verevon SPA to *initiate* sessions; the customer-facing Verevon Agent always registers over the native UDP port (21116).
- **`force_relay: true`** is the correct flag for a browser-only controller to set on its `PunchHoleRequest` — it tells hbbs to skip P2P/hole-punch negotiation entirely, which a browser could never do anyway. Set `ALWAYS_USE_RELAY=Y` on the server side too, so this isn't solely dependent on every client remembering to ask for it.

## Configuration

See `.env.example` for the full list. The load-bearing ones:

| Variable | Purpose |
|---|---|
| `SUPPORT_PLANE_DOMAIN` | Public DNS name Caddy requests a certificate for (e.g. `support.verevon.com`) |
| `SUPPORT_PLANE_RELAY_ADDR` | Advertised relay address (`RELAY` env var passed to hbbs) — must be the address clients can actually reach hbbr on |
| `SUPPORT_PLANE_KEY` | The `-k` license/access key both hbbs and hbbr require to match — generate a real secret per environment, never reuse the local-dev default |

## Connecting the Verevon gateway to this plane

The SPA never hardcodes these addresses: the gateway's `remote_support` domain
(`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/remote_support.rs`)
serves them from `GET /api/v1/remote-support/config`. Set these on the
**gateway**, not on the SPA:

| Gateway variable | Value |
|---|---|
| `REMOTE_SUPPORT_RENDEZVOUS_URL` | `wss://<SUPPORT_PLANE_RENDEZVOUS_DOMAIN>` (Caddy fronts hbbs:21118) — `ws://localhost:21118` for a local rig |
| `REMOTE_SUPPORT_RELAY_URL` | `wss://<SUPPORT_PLANE_RELAY_DOMAIN>` (Caddy fronts hbbr:21119). Optional; overrides the address hbbs advertises |
| `REMOTE_SUPPORT_SERVER_PUBLIC_KEY` | The `Key:` value hbbs logs at first start (see above). **Required** — without it the gateway reports `configured: false` and the UI refuses to connect, because the peer could not be authenticated |

The gateway reports `configured: false` plus the missing variable names rather
than inventing a default host, and the Support › Remote support surface shows
exactly that state until all required values are present.

## What Verevon still needs to build on top of this

Per `remote-core`'s own architecture doc, the connect flow through this infrastructure is: Verevon's backend issues a short-lived session token → the browser passes it as `auth.token` to `@verevon/remote-core` → it's used exactly like a RustDesk temporary password (`RustDeskPasswordAuthenticator`, already implemented and tested in `remote-core`). **Support Plane itself does not know anything about Verevon sessions, tickets, or users** — that mapping (which support agent is allowed to reach which customer's device ID, issuing/rotating temporary passwords on the Verevon Agent side) is Convex/Control-Plane-owned product logic, not something this infrastructure plane does.

## Local development

```bash
cd "apps/Support Plane"
cp .env.example .env   # fill in real values before running anywhere but localhost
docker compose up -d hbbs hbbr readiness   # omit Caddy locally: it wants real DNS + certs
docker compose ps
```

`SUPPORT_PLANE_KEY=_` only works for a **rendezvous-only** smoke test (hbbs alone, no relay). It is **not** fine for anything that actually connects a peer: hbbs and hbbr each run `_` through their own independent keypair generation (`gen_sk`, on their own separate `hbbs-data`/`hbbr-data` volumes), so they end up with two *different* keys. hbbs will still answer rendezvous requests, but the moment a session needs the relay, hbbr rejects it with "invalid key" (`relay_server.rs`: `Relay authentication failed … - invalid key`) — confirmed live on 2026-09-10 while connecting a real RustDesk client through this exact stack.

Use a real, explicit, **shared** `SUPPORT_PLANE_KEY` instead — the same value for both services, which the compose file already wires up identically. It must be a base64-encoded 64-byte Ed25519 **secret** key (libsodium/sodiumoxide's format: 32-byte seed + 32-byte public key), not an arbitrary string: `rustdesk-server`'s `get_server_sk()` only derives a *shared, deterministic* public key from a value shaped like that; an arbitrary string is used as a literal access-key string instead; and neither hbbs nor hbbr would use the real signing key their code path assumes elsewhere. Generate one with `remote-core`'s own crypto dependency (from `apps/Frontend Plane/verevonv3/packages/remote-core`, where `libsodium-wrappers` is already installed):

```bash
node -e "
const sodium = require('libsodium-wrappers');
(async () => {
  await sodium.ready;
  const kp = sodium.crypto_sign_keypair();
  console.log('SUPPORT_PLANE_KEY=' + sodium.to_base64(kp.privateKey, sodium.base64_variants.ORIGINAL));
})();
"
```

Put that value in `.env` (never `_`, never a plain password string) and recreate hbbs/hbbr.

### Getting the server key clients need

With a real, explicit `SUPPORT_PLANE_KEY` set, both hbbs and hbbr log the **same** derived public key at startup:

```bash
docker compose logs hbbs | grep 'Key:'
docker compose logs hbbr | grep 'Key:'   # must match hbbs's line exactly
```

That single base64 string is **both** the access key clients must present *and* the key used to verify peer identity — `remote-core` takes it as `serverPublicKey` and reuses it as `licenceKey` automatically. Verified empirically: send an empty key and hbbs answers `LICENSE_MISMATCH`; send the wrong one and hbbr answers "invalid key" (see above).

### Reaching the WebSocket ports locally

Ports 21118/21119 are deliberately **not** published to the host — only Caddy (same Docker network) may reach them, because they are plaintext `ws://`. For local testing either run your client inside the network:

```bash
docker run --rm --network support-plane_support-plane node:24-alpine ...
```

or add a throwaway override that publishes them to `127.0.0.1` only. Do not publish them in any shared environment.

### A note on health

The `rustdesk-server` image is built `FROM scratch` — it has no `/bin/sh` and no `nc`, so an in-container healthcheck is impossible (it fails with `stat /bin/sh: no such file or directory` and marks a perfectly healthy service as unhealthy). That is why hbbs/hbbr carry no healthcheck and a small `readiness` sidecar TCP-probes all four ports over the network instead. Gate on `readiness`, not on hbbs/hbbr.

## Known gaps

- No production TLS certificate has been provisioned or verified in this pass — Caddy is configured to request one automatically on first real deployment, but that has not been exercised.
- This has been done manually once (a stock RustDesk client standing in for the Verevon Agent, see Status above), but no *automated* integration test exists yet, and no real Verevon Agent exists — see `remote-core`'s `docs/architecture.md`, "Status," for what has and hasn't been verified end-to-end, and "Next verification step" for what the manual pass did not settle (it did not exercise pointer, keyboard, scroll, or cursor).
- Rate limiting / abuse protection beyond Caddy's defaults has not been configured.
