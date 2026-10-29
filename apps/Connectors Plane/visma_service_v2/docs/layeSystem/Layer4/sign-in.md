# Signing in as yourself (`vsv2-mcp login`)

By default the MCP server reads Visma with the integration's **service credentials**: whoever can call it sees
every configured company. After `vsv2-mcp login` it acts as **you**: each live read and each write uses a token that
Visma issued to you for one company, so Visma applies your own roles, and the server can only see the companies you
connected. This is the per-user identity the official Visma MCP has (see [`parity.md`](parity.md)).

| | Service credentials (default) | Signed in |
|---|---|---|
| Live reads (`visma_erp_query`, `visma_salesorder_get`) | every configured company | only the companies you connected, with your permissions |
| Writes | the service's write scope | your token; needs `login --write` and the SalesOrders write scope |
| DataMart (`visma_datamart_*`) | yes | **off**: it has no per-person sign-in |
| `visma_list_tenants` | the configured companies | your connected companies (named by Visma if not configured) |
| Default company (`tenant` omitted) | Aquatiq AS | Aquatiq AS if you connected it, otherwise none: the call must name a company |
| Audit log `caller` | `mcp:$USER` (a label) | `visma-user:<your e-mail, else name, else id>` as Visma Connect gave it |
| Warehouse (`warehouse_*`) | this computer's `gcloud` login | unchanged: still this computer's `gcloud` login |

## How the sign-in works

Visma Connect issues an access token for **one company per approval**: Visma's own guide says a user with several
companies "will choose which to connect to your application upon each single sign-on flow". So signing in to all your
companies means approving each one. `vsv2-mcp login` does that in one go, the way the Xero integration combines
tenants, except that Visma cannot hand over a list:

1. It opens Visma's consent screen (authorization code + PKCE) and waits for the redirect on `localhost`.
2. After you approve a company it trades the code for tokens, stores them for that company, and answers the
   redirect with a redirect to the **next** consent screen, in the **same browser tab**.
3. It repeats until Visma offers a company you already connected in this run (there are no more), you decline, you
   press **Enter** in the terminal, or nobody answers within five minutes (`--timeout-secs`). Companies connected
   before that point stay connected. `--configured-only` also stops once every active company in `config/tenants` is
   connected; `--once` connects one.

Each company has its own refresh token. Tokens are renewed when they run out, under a per-company lock; a refresh
token that another process rotated a moment earlier is picked up from disk rather than failing.

## One-time setup in the Visma Developer Portal

Create a **web application** (server-side; not a single-page or native app, which do not get refresh tokens) and:

- allow `offline_access` (without it the sign-in would last about an hour and `login` refuses it),
- register the redirect URL exactly as `VISMA_USER_REDIRECT_URI`, by default `http://localhost:38271/callback`,
- give it the Finance interactive API (`vismanet_erp_interactive_api:read`) and, for writes, the SalesOrders API
  scopes in `VISMA_USER_SCOPES_WRITE`.

Then put the application's credentials in `.env` (see `.env.example`):

| Variable | Default | |
|---|---|---|
| `VISMA_USER_CLIENT_ID`, `VISMA_USER_CLIENT_SECRET` | required | the web application |
| `VISMA_USER_REDIRECT_URI` | `http://localhost:38271/callback` | loopback `http` (a listener catches it) or an `https` URL of yours (then `login` asks you to paste the address you land on) |
| `VISMA_USER_SCOPES` | `openid profile email offline_access vismanet_erp_interactive_api:read` | everything asked for at sign-in |
| `VISMA_USER_SCOPES_WRITE` | `visma.net.erp.salesorder:read visma.net.erp.salesorder:write` | added by `login --write` |
| `VISMA_USER_PROMPT` | `consent` | sent as `prompt`; set it empty to send none |
| `VSV2_MCP_IDENTITY` | `auto` | `auto`: you once signed in, else the service. `user`: you only, and off without a sign-in. `service`: ignore any sign-in |
| `VSV2_USER_STORE_DIR` | `$HOME/.vsv2/user` | where the sign-in is kept |

The default scopes are the Finance read scope only, so that a first sign-in cannot fail on a scope the application is not
allowed to use. Reads of sales orders (`visma_salesorder_get`, `sales_orders_v3`) go to the SalesOrders API and need
`visma.net.erp.salesorder:read` as well: add it to `VISMA_USER_SCOPES` once the application may use it (`login --write` adds
it together with the write scope). A call whose scope the sign-in lacks is refused before Visma is asked, with that
scope named.

User mode needs none of the sync service's secrets. `VSV2_DATABASE_URL` is needed only for `visma_sync_status` and for
writes (the audit log).

## Commands

```bash
scripts/run_mcp.sh login [--write] [--once] [--configured-only] [--paste] [--timeout-secs N]
scripts/run_mcp.sh whoami [--check]      # who is signed in, which companies, which scopes; --check asks Visma if each is still accepted
scripts/run_mcp.sh logout [--company ID] # revoke at Visma and forget; with no companies left, the service credentials apply again
```

`scripts/run_mcp.sh` runs the release binary from the repository root, so `.env` and `config/` resolve (build it first:
`cargo build --release -p vsv2-mcp`). Throughout this guide `vsv2-mcp login` means that command.

Start the MCP server as usual afterwards; a server that is already running does not notice a new sign-in or a
logout, so start a new session (a new Claude Code session, or restart the client). `visma_init` states whose authority applies (`acts_as`, and
`capabilities.identity` with the companies). A sign-in that exists but cannot be used (its application is not
configured, the file is unreadable, another application made it) turns the live tools **off with the reason**. It
never falls back to the service credentials, which can see more than you.

## What is stored, and how it is protected

- `~/.vsv2/user/user.json` (who) and `connections/<company id>.json` (tokens for one company), directories `0700`,
  files `0600`, written through a temporary file and a rename. **Tokens are not encrypted at rest**: protect the
  account and the disk as you would an SSH key. A loosened file is tightened when read.
- One person per computer: a second person's approval is refused until `logout`.
- The authorization request carries a fresh `state` and PKCE pair; the answer must carry that `state` and, when
  Visma sends one, Visma Connect as `iss`. The listener binds to loopback only, answers only `GET` on the redirect
  path for a loopback `Host`, and ignores everything else. Tokens, codes and the client secret are never logged or
  echoed in errors, and error bodies are bounded and stripped of control characters.
- Token and revocation endpoints must be `https` on a `*.visma.com` / `*.visma.net` host (loopback only for tests).

## Limits, on purpose and by default

- **One person per computer.** A second person's approval is refused until `logout`. Every stored connection carries the
  person it belongs to, so a server that is already running refuses tokens of somebody else (after a logout and a login
  as another person) instead of using them under the old name; it asks for a restart.
- **`auto` falls back to the service credentials when nobody is signed in**, including when the store cannot be located.
  On a computer where nobody should ever act with the service credentials, set `VSV2_MCP_IDENTITY=user`.
- **Refreshing is not locked between processes.** Two servers renewing the same company at once is tolerated (the one
  that loses re-reads the store, twice, 250 ms apart), but a winner that saves more than a moment late can make the loser
  report "expired or revoked" once; the next call works. Tokens that could not be written to disk are kept in memory by
  the process that got them, and written when the disk works again; if that process ends first, sign in again.
- **Loopback endpoints are accepted** for the authorize, token, revocation, userinfo and Finance URLs (and the
  SalesOrders and Finance base URLs of the server), so tests can use a mock. They are read from the environment and from
  the `.env` that is found by looking in the current directory and its parents: start through `scripts/run_mcp.sh`
  (which fixes the directory), and do not run `login` in a directory whose `.env` you do not trust.
- **Tokens are not encrypted at rest and the permission tightening is Unix-only.** Keep the store on a protected disk;
  on Windows no ACL is set.
- **`visma_sync_status` shows only your companies** in user mode (it reads the shared sync database with this computer's
  own credentials); the warehouse tools use this computer's `gcloud` login.
- **The browser is opened on a one-time local address** that redirects to Visma, so the authorization URL (which holds
  the `state` and the PKCE challenge) does not appear in a process list. The full URL is still printed in your terminal.

## Verified, and not verified

Verified by tests against a mock Visma Connect (`cargo test -p vsv2-signin`, `-p vsv2-mcp --test user_mode_e2e`):
the whole loop in one tab, decline / timeout / Enter / repeat / paste, a second person refused, refresh and rotation,
per-company tokens through the real binary, DataMart off, no fallback to the service identity. Verified against
Visma: Connect's discovery document (authorization code, refresh token, PKCE `S256`, `offline_access`, device
grant); that the Finance gateway has **no** `/erp/interactive` or `/erp/service` route (404), so a signed-in person
calls the same host and paths as the service and the "API base URL" in the portal is the token's audience; and
`GET /v1/organization` returning the company name.

**Not verified, because it needs you at the consent screen:**

- how Visma's company chooser behaves after the first company (the screen lists companies as "Already connected");
  the loop stops safely if it offers nothing new, but whether it lets you pick an already connected company again is
  unknown. If it does not, `login` ends with "Visma offered no further company".
- that the portal accepts the redirect URL, and that the application may use `offline_access`;
- whether the SalesOrders API accepts a person's token and its scopes (its published swagger documents client
  credentials only). Reads of `sales_orders_v3` and every write operation go to that API; if Visma refuses, the call
  fails with Visma's answer and nothing else is tried.

## Problems

| You see | Cause | Fix |
|---|---|---|
| `invalid_scope` after the consent screen | the application may not use a scope in `VISMA_USER_SCOPES` | allow it in the portal (web app, `offline_access`) or remove it |
| `unauthorized_client` / `invalid_request` | wrong client id, or the redirect URL is not registered exactly | check both |
| `cannot listen on 127.0.0.1:38271` | port in use | change the port in `VISMA_USER_REDIRECT_URI` and the portal, or `login --paste` |
| `returned no refresh token` | `offline_access` missing or not allowed | add it |
| `the sign-in does not include the scope ...` | the sign-in lacks a scope the call needs | `vsv2-mcp login --write` (or edit the scope variables) |
| `expired or revoked` | the refresh token is gone | `vsv2-mcp login` again |
| `visma_init`: identity disabled | see its `reason` | fix it, or `vsv2-mcp logout` to use the service credentials |
