# Official Xero MCP audit

Audited source: `vendor/xero-mcp-server` at upstream commit
`f24583c867df7c1f4806f7532eb0691e85892865` (2026-06-05). This is an audit
of the vendored upstream code, not a description of Aquatiq's Xero MCP.

## What it is

The upstream project is a small Node.js/TypeScript MCP **stdio** server. Its
entry point creates one `McpServer`, registers fixed tool definitions, and
connects `stdin` and `stdout` using `StdioServerTransport`. It has no HTTP,
SSE, Streamable HTTP, OAuth authorization-server, Dynamic Client Registration,
or browser-facing sign-in endpoint.

Each tool calls a handler. The handler authenticates a process-global
`xero-node` SDK client, calls one Xero Accounting or Payroll NZ API method,
then returns LLM-oriented text. Some writes include a Xero UI deep link.

Architecture:

```text
MCP host
  -> stdio
  -> fixed Zod tool schema
  -> handler
  -> process-global Xero SDK client
  -> Xero Accounting API or Payroll NZ API
  -> formatted text response
```

The important source files are:

| Area | Source |
| --- | --- |
| Stdio bootstrap | `src/index.ts` |
| MCP server metadata | `src/server/xero-mcp-server.ts` |
| Tool registration | `src/tools/tool-factory.ts` |
| Credential and tenant client | `src/clients/xero-client.ts` |
| Tool implementations | `src/tools/` |
| API handlers | `src/handlers/` |
| Error redaction | `src/helpers/format-error.ts` |

## Authentication and tenant behavior

Two mutually exclusive environment modes are supported:

| Mode | Inputs | Behavior |
| --- | --- | --- |
| Custom Connection | `XERO_CLIENT_ID`, `XERO_CLIENT_SECRET`, optional `XERO_SCOPES` | Requests a client-credentials token, first using legacy bundled scopes and then granular scopes after an `invalid_scope` response. It calls `/connections` and selects the first tenant. |
| Bearer token | `XERO_CLIENT_BEARER_TOKEN` | Places the supplied token into the SDK, calls `updateTenants`, and selects the first tenant. Token acquisition and refresh are external responsibilities. |

The client is process-global. There is no tenant parameter on any tool, no
tenant allowlist, user identity, per-client authorization, token store, token
rotation coordinator, or audit record. A bearer token connected to multiple
organizations silently operates on `tenants[0]`.

## MCP capabilities

It registers **51 tools**: 25 list/read tools, 11 create tools, 13 update or
workflow tools, one direct get tool, and one delete tool. It does not expose
MCP resources, resource templates, prompts, sampling, elicitation, skills, or
a plugin/extension API.

### Read tools (25)

Accounting and reports:

```text
list-accounts
list-aged-payables-by-contact
list-aged-receivables-by-contact
list-bank-transactions
list-contact-groups
list-contacts
list-credit-notes
list-invoices
list-items
list-manual-journals
list-organisation-details
list-payments
list-profit-and-loss
list-quotes
list-report-balance-sheet
list-tax-rates
list-tracking-categories
list-trial-balance
```

Payroll NZ:

```text
get-timesheet
list-payroll-employee-leave
list-payroll-employee-leave-balances
list-payroll-employee-leave-types
list-payroll-employees
list-payroll-leave-periods
list-payroll-leave-types
list-timesheets
```

Most list handlers return only one page. Several Accounting lists hard-code a
page size of ten, and response content is emitted as multiple text blocks or
pretty-printed JSON. There is no retained result handle, cursor abstraction,
bounded result envelope, or raw extraction job.

### Create tools (11)

```text
create-bank-transaction
create-contact
create-credit-note
create-invoice
create-item
create-manual-journal
create-payment
create-quote
create-timesheet
create-tracking-category
create-tracking-options
```

Invoice creation defaults to `DRAFT` and calculates a 30-day due date when the
caller does not provide one. Contact, invoice, credit note, manual journal,
quote, and payment responses can include a Xero UI deep link.

### Update and workflow tools (13)

```text
add-timesheet-line
approve-timesheet
revert-timesheet
update-bank-transaction
update-contact
update-credit-note
update-invoice
update-item
update-manual-journal
update-quote
update-timesheet-line
update-tracking-category
update-tracking-options
```

### Delete tool (1)

```text
delete-timesheet
```

Writes are executed immediately by the MCP call. There is no prepare/inspect/
approve step, no durable receipt, and no server-generated idempotency key.

## Integrations, examples, skills, and plugins

There is no plugin protocol or skills package in the repository. The project
has these integrations only:

| Integration | Role |
| --- | --- |
| `@modelcontextprotocol/sdk` | MCP server and stdio transport. |
| `xero-node` | Xero Accounting and Payroll NZ SDK clients. |
| `axios` through the dependency graph | Custom Connection token and connection discovery calls. |
| `dotenv` | Local environment-file loading. |
| `zod` | Per-tool argument schemas. |
| `examples/openai-agents` | A small Python example that launches this server through stdio for OpenAI Agents SDK. |
| Claude Desktop configuration | README-only local `npx` configuration. |

The OpenAI example is an MCP *host* example, not an agent skill, plugin, or
production orchestration layer. `glama.json` only supplies maintainer metadata.

## API coverage

The upstream server implements a curated Accounting and Payroll NZ workflow,
not full Xero OpenAPI coverage. It does not implement Assets, Bank Feeds,
Finance, Files, Identity, Projects, Payment Services, attachments, generic
report discovery, report-by-ID, webhooks, or arbitrary documented GET
operations.

Aquatiq's MCP contains every upstream read and write operation as a contract,
but has a different and safer surface: Xero OpenAPI operation IDs plus explicit
tenant UUIDs for reads, and an allowlisted `official_mcp` write form that
requires an exact action grant and independent operator approval.

## Material implementation findings

### High: writes have no idempotency or approval control

Seventeen Accounting write calls pass `undefined` for the Xero idempotency
parameter. All writes are executed as soon as an MCP client invokes a tool. A
network timeout can therefore leave the caller uncertain whether a financial
record was created and encourage a duplicate retry.

Aquatiq should retain its current prepare, inspect, operator approval, durable
receipt, and server-generated `Idempotency-Key` design. Do not copy this
upstream execution model into the hosted connector.

### High: two tracking creation handlers do not await authentication

`create-xero-tracking-category.handler.ts` and
`create-xero-tracking-option.handler.ts` call `xeroClient.authenticate()`
without `await`. The Xero request can run before the token and tenant are set.
This is a real race in the vendored source and should not be reused.

### High: profit-and-loss parameters are wired incorrectly

`listXeroProfitAndLoss` accepts both `standardLayout` and `paymentsOnly`, but
passes `paymentsOnly` into the `standardLayout` position when it calls
`fetchProfitAndLoss`. The actual `paymentsOnly` argument is omitted. A request
for payment-only reporting can change the layout instead, while
`standardLayout` is ignored.

### Medium: first-tenant selection is unsafe for a multi-organization service

Both authentication modes select the first connection. There is no explicit
tenant choice or authorization check, so it is unsuitable as the core of a
multi-tenant remote service.

### Medium: no token refresh, retry, rate limiter, or durable state

Custom Connections request a new token for every `authenticate()` call. Bearer
tokens are static and must be refreshed outside the server. The project does
not coordinate concurrent refreshes, retry 429/5xx responses, persist jobs or
results, track idempotency, or audit requests.

### Medium: weak test coverage

The repository contains one test file for error formatting. It has no tests
for the client, tool registration, parameter forwarding, writes, tenant
selection, authentication, or an MCP protocol round trip.

### Medium: committed clean install is currently not reproducible

At the audited revision, `npm ci --ignore-scripts` fails before a build starts:
the committed `package-lock.json` is inconsistent with dependency resolution
for the `@emnapi` packages. The upstream maintainers need to regenerate and
commit the lockfile using their supported npm version. This does not alter
Aquatiq's runtime, but it prevents treating that revision as a reproducible
vendor build without a temporary non-lockfile install.

### Low: documentation and implementation differ on payroll geography

The README says Payroll-specific queries require NZ or UK. The code invokes
`payrollNZApi` exclusively; there is no Payroll UK client or tool.

## Reuse guidance for Aquatiq

Reuse only the narrow, low-risk ideas:

- Zod-style input validation and well-named financial workflow tools.
- Xero UI deep links as optional response metadata.
- Sanitized, user-meaningful Xero error mapping.
- The useful curated Payroll NZ workflow list.

Keep Aquatiq's existing advantages:

- Server-side PKCE and refresh-token storage.
- Explicit authorized tenant selection.
- OpenAPI-pinned endpoint catalog and broader API coverage.
- Compact discovery and bounded result handling.
- Raw extraction, BigQuery warehouse controls, and data-quality workflows.
- Approval-gated, idempotent writes with receipts.

Before adopting a deep-link response, make it optional and return it as a
structured link rather than adding it to a free-form text response.
