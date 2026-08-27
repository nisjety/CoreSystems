# Verevon v3

> **Model Plane production-readiness correction — 2026-07-13:** the running
> Model Gateway and inference gRPC listeners are absent despite green HTTP
> health. Frontend/Auth Core source now uses separate exact-audience credentials
> for Model, inference, execution, session, capability, cost, and Data Plane
> calls and fails required issuance with 503. Plain chat remains intentionally
> tool-free; explicit actions/Browse/Plan are the user-controlled entry points.
> The authoritative capability-state/UX work is not complete, and shipping is
> not yet a selectable chat action. See
> [the cross-plane handoff](docs/MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md).

**Live verification:** 2026-07-10 · Dockerized Vite frontend (`:5173`) and Rust gateway (`:3185`)

**Source re-verification:** 2026-07-11 (source-only; SPA/gateway containers were down this pass) · stack versions, scripts, `vite-plugin-mcp`, zod action schemas, and the `src/shared/*` boundaries confirmed against current source. Corrected the transport-directory reference in Boundaries: `src/shared/rpc` and `src/shared/graphrest` no longer exist — all transport clients live under `src/shared/api`.

Standalone SolidJS + Vite + TypeScript frontend for Verevon, created beside `verevonv2` in the Frontend Plane.

## Stack

- SolidJS 2.0 (release candidate) for fine-grained client rendering.
- Vite 8 for dev/build speed and static client deployment.
- TypeScript 7 strict mode with path aliases.
- `vite-plugin-mcp` enabled in `vite.config.ts`; Vite exposes the local MCP endpoint at `/__mcp/sse` during development.
- `zod` action schemas used by human UI controls and selectively projected into Model Plane calls.

## Boundaries

- `src/app` owns routing, providers, and the application shell.
- `src/features/*` owns each product surface: dashboard, chat, inbox, agents, knowledge, onboarding, settings.
- `src/shared/actions` is the AI-first command registry. Every meaningful UI operation should be represented here before a feature calls it. The live audit found that the frontend registry, direct Model Gateway tools, and execution-core tools are not yet one runtime catalog; see the current audit addendum below.
- `src/shared/context-packs` builds compact model context from route, visible records, draft input, and available actions.
- `src/shared/api` holds the API-first transport clients (56 files) for the Rust same-origin gateway and cross-plane services. (Verified 2026-07-11: the previously advertised `src/shared/rpc` and `src/shared/graphrest` directories do not exist; RPC and graph shapes are served through the gateway via these `api` clients.)
- `src/shared/cost` owns model routing policy for cost-aware Model Plane calls.

## Setup

One prerequisite is not satisfied by `pnpm install`, and it fails in a way
that does not name itself. `package.json` depends on `@quarry/client` by
`file:` path, but that SDK is **generated** from Quarry's OpenAPI spec and
Quarry's `.gitignore` keeps everything except its markdown docs out of the
repo. On a fresh checkout the directory therefore has no `package.json`, so
`pnpm install` cannot resolve the dependency at all -- and even once it can,
the package's `types` field points at `dist/`, so the SDK must also be
compiled before `tsc` can see a single one of its types.

Generate and build it once, before the first install:

```bash
# one command per line: `&&` is not a statement separator in Windows PowerShell
cd "../../Ingestion Plane/Quarry-v2/sdks"
bash ./generate.sh            # requires a running Docker daemon
cd typescript
pnpm install --ignore-scripts
pnpm run build
```

Re-run it whenever `Quarry-v2/docs/openapi.yaml` changes; CI does this on
every run (see `.github/workflows/verevonv3-ci.yml`), which is why the
workflow is also path-triggered on that spec.

## Scripts

```bash
pnpm dev
pnpm lint
pnpm typecheck
pnpm test
pnpm build
pnpm verify
```

Docker Compose defaults to the `dev` image target with the source bind-mounted
for Vite HMR. The production override builds the static SPA once, serves it as
an unprivileged nginx process, removes source mounts, and keeps the Rust gateway
internal to the shared network:

```bash
docker compose -f docker-compose.yml -f docker-compose.production.yml up -d --build
```

## Live verification — 2026-07-10

The Docker stack had 91 running containers and no unhealthy containers when checked. Representative health/readiness probes returned HTTP 200 for the frontend, gateway, Model, Control, Data, Ingestion, and Application services. The authenticated cross-plane Playwright smoke suite passed 6/6 against `http://localhost:5173` using the seeded local account.

The current behavior is deliberately recorded here because “running” does not mean “AI-accessible”:

- Normal chat reaches live Azure GPT-4o-mini inference, but sends zero tools and zero citations by default. It therefore cannot discover shipping-core, MCP, Control, or Data capabilities.
- Browse mode successfully called live web search/fetch and returned five citations.
- Knowledge search was reachable but returned an honest empty result for the seeded organization.
- Shipping is mixed live/demo: Bring, DHL, UPS, and FedEx are registered as non-mock adapters, while PostNord, DSV, Helthjem, and Porterbuddy are explicit mocks. Bring pricing is live, but its transit-time fields currently arrive as zero because the adapter parses the wrong response fields.
- The live audit found that the human action registry, direct Model Gateway tools, and execution-core Plan catalog are separate catalogs. Do not describe all UI operations as automatically available to chat until the catalogs are unified.

Safe verification commands:

```bash
docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
E2E_BASE_URL=http://localhost:5173 E2E_EMAIL=local@verevon.dev E2E_PASSWORD='<seeded-local-password>' \
  pnpm exec playwright test tests/e2e/cross-plane-smoke.spec.ts --project=e2e
curl http://localhost:3185/health
curl http://localhost:3156/api/carriers
```

The dated cross-plane evidence and remaining security findings are recorded in `docs/core-research/plane-audit-2026-07-02.md` under the 2026-07-10 live-verification addendum.

## Rule for New Features

Add the action contract first, then render the UI control that calls it. The user and the AI should use the same action ID, input schema, approval rule, and audit path.
