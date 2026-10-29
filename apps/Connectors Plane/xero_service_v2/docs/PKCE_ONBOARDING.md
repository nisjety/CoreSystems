# Xero PKCE Onboarding — SolidJS SPA + Rust BFF

Self-serve "Connect your Xero" onboarding. A user authorizes one or more Xero
organisations via OAuth2 **Authorization Code + PKCE**; the gateway captures a
**refresh token + the connected tenants** and persists them so the sync backend
can keep those orgs up to date. This is the user-facing alternative to the
manual per-org custom-connection setup used by the uploader.

> Auth model note: the raw→GCS uploader uses the **custom-connection**
> (`client_credentials`) grant (see [.env.example](../.env.example)). This
> onboarding flow is **PKCE** — a different Xero app and a different grant. They
> coexist; neither replaces the other.

## Architecture

```
SolidJS SPA  (web/, served on :5002)         Rust BFF  (xero-bff, axum :5002)          Xero
  "Connect Xero"  ──GET /api/auth/login────────▶ mint state + PKCE verifier
                                                  (encrypted cookie)  ──303──▶ login.xero.com/authorize
  user signs in, picks org(s)  ◀───────────────────────────────────────────────────────┘
  browser  ──GET /api/xero/callback?code&state─▶ verify state
                                                  → POST /connect/token (code + verifier, no secret)
                                                  → GET /connections
                                                  → persist refresh_token + tenants (ConnectionStore)
                                                  → set session cookie  ──303──▶ SPA /connected
  /connected  ──GET /api/connections───────────▶ list connected tenants (no tokens)
```

The local Compose setup serves the built SPA and BFF on port 5002, so the
browser is **single origin** (the session cookie is sent back). Refresh tokens
and the client id never reach the browser.

## Components

| Path | What |
|---|---|
| `web/` | SolidJS + Vite + TS SPA (`@solidjs/router`, `@tanstack/solid-query`). Routes: `/` connect, `/connected` list, `/auth/error`. |
| `core/crates/xero-bff/` | axum gateway: `/api/auth/login`, `/api/xero/callback` (and legacy `/api/auth/callback` alias), `/api/connections`, `/api/auth/logout`, `/api/health`. |
| `core/crates/xero-auth/src/authorization_code.rs` | PKCE helpers: challenge/verifier, authorize URL, token exchange, refresh, `/connections`. |
| `xero-bff` `ConnectionStore` | Trait + `FileConnectionStore` (dev, JSON, owner-only). Swap for Secret Manager / encrypted GCS in prod. |

## Prerequisite — the Xero PKCE app

This project **reuses the v1 service's Xero app** (a confidential "Web app":
Authorization Code + PKCE **with a client secret**). Its credentials were copied
into `.env.bff` (`XERO_PKCE_CLIENT_ID`, `XERO_PKCE_CLIENT_SECRET`, `XERO_PKCE_SCOPES`).
Quote `XERO_PKCE_SCOPES` in the env file because it contains spaces; the BFF
now fails startup if a present `.env.bff` cannot be parsed.

The local BFF uses the app's registered
`http://localhost:5002/api/xero/callback` redirect URI. The single-origin
Compose setup maps host port 5002 to the container's internal port 8080;
standalone BFF development binds port 5002 directly. Register any additional
redirect URI in the Xero developer portal before selecting it in configuration.

To use a *new* app instead: create one at https://developer.xero.com, put its
client id/secret in `.env.bff`, and register the redirect URI above. A pure
public (mobile) app works too — just leave `XERO_PKCE_CLIENT_SECRET` unset.

## Local development

```bash
# 1) Build the SPA and run the BFF from the repo root. .env.bff is auto-loaded.
#    Regenerate the cookie key if needed:
#    openssl rand -base64 64   →   XERO_BFF_COOKIE_KEY  in .env.bff
cd web
pnpm install
pnpm build
cd ..
XERO_BFF_STATIC_DIR=web/dist cargo run -p xero-bff
```

Open http://localhost:5002, click **Connect to Xero**, authorize, and you land
on `/connected` listing the org(s). The refresh token is written to
`./data/connections.json` (gitignored, `chmod 600`).

If an older attempt ended at `missing_state_cookie`, start a new login from
`http://localhost:5002`. The callback is registered on `localhost`, and PKCE
state is held in a host-only cookie. The BFF redirects a local login opened at
`127.0.0.1:5002` to `localhost:5002` before creating that cookie. An existing
Xero authorization URL cannot be resumed after its state cookie is lost.

## Environment

See [.env.bff.example](../.env.bff.example). Required: `XERO_PKCE_CLIENT_ID`,
`XERO_REDIRECT_URI`, `XERO_BFF_COOKIE_KEY`. Everything else has a default.

## API contract

| Method + path | Auth | Response |
|---|---|---|
| `GET /api/health` | — | `ok` |
| `GET /api/auth/login` | — | 303 → Xero authorize (sets encrypted `xero_pkce` cookie) |
| `GET /api/xero/callback?code&state` | pending cookie | 303 → SPA `/connected` or `/auth/error?reason=<slug>` |
| `GET /api/connections` | session cookie | `{ "connections": [{ tenantId, tenantName, tenantType, connectedAt, scopes }] }` (401 without session) |
| `POST /api/auth/logout` | — | 204 (clears session) |

Error `reason` slugs: `access_denied`, `missing_code`, `missing_state_cookie`,
`bad_state_cookie`, `state_mismatch`, `token_exchange_failed`,
`no_refresh_token`, `connections_failed`, `no_connections`, `store_failed`.

## Security

- Refresh tokens & client id stay server-side; the SPA only sees tenant metadata.
- Cookies are **encrypted** (`PrivateCookieJar`), `HttpOnly`, `SameSite=Lax`;
  set `XERO_BFF_SECURE_COOKIES=true` behind HTTPS.
- CSRF: `state` is checked on callback; PKCE `S256` binds the code to the verifier.
- The dev `FileConnectionStore` writes refresh tokens to a `chmod 600` JSON file.
  It is gitignored — **do not commit `data/connections.json`**.

## Production notes

- **Store**: replace `FileConnectionStore` with a Secret Manager / encrypted-GCS
  implementation of the `ConnectionStore` trait — refresh tokens are long-lived.
- **Serving**: build the SPA (`pnpm --dir web build` → `web/dist`) and serve it
  behind the same origin as the BFF (reverse proxy, or add a `tower-http`
  `ServeDir` fallback), so `/api/*` and the SPA share an origin. Update
  `XERO_REDIRECT_URI` to the prod `https://…/api/auth/callback` and register it
  on the Xero app.
- **Consuming the tokens**: the sync backend reads refresh tokens from the same
  store (`ConnectionStore::get_refresh_token`) and calls
  `xero_auth::authorization_code::refresh(...)` to mint access tokens per tenant.
