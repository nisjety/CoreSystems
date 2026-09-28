# Learn Core — Teachme clients

Independent frontend workspace for CoreSystem's Learning Plane.

| Application | Technology | Local URL / command |
|---|---|---|
| Marketing | Next.js 16 / React | `pnpm dev:marketing` — http://127.0.0.1:3201 |
| Dashboard | Vite 8 / Solid 2 RC | `pnpm dev:dashboard` — http://127.0.0.1:5201 |
| Windows | .NET 10 / C# / WPF | See `desktop/windows/README.md` |

```powershell
cd 'C:\dev\CoresSystem\apps\Frontend Plane\learn-core'
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
pnpm build
```

Start Learning Core separately as described in [Learning Plane](../../learning-plane/README.md). Vite proxies `/api/auth/*` to Control Auth at `127.0.0.1:3011` and `/api/v1/learning/*` to Learning Core at `127.0.0.1:3196`. Both are same-origin browser requests in this local setup. The production BFF, trusted origins, session handling and school entitlement policy still require deployment configuration and review.

Solid 2 and its Vite plugin are prereleases pinned together. No changes to Verevon's dependency tree are required. `packages/learning-client` contains framework-independent status validation/client code that can be consumed by Verevon later.

## Current behavior

The dashboard supports existing Control accounts, class and membership administration, course drafts, immutable published revisions and revision-pinned class assignments when the Learning service is fully configured. Teachers can inspect assignment attempt summaries. Those summaries count learner-reported step completions and do not prove independent mastery. Otherwise the service remains closed in foundation mode. It does not fabricate students, course completion, remote sessions or authentication. Marketing describes the proposed pilot and links to this development dashboard. The Windows app opens a local request window; it does not capture, transmit or control a screen.

This L01 source path has not yet been accepted against a live disposable school tenant. Audit/outbox and notifications, Learn product entitlement, directory/invitation UX, student-facing attempt flow, native guidance integration, and cross-tenant/revocation/restart scenarios remain open. Do not use real student data.

Default product policy is strict self-hosting. Whisper is selected for recognition; no speech/model provider is connected. Native capture, teacher device access and installing a signed school client remain gated by the [implementation plan](../../learning-plane/docs/IMPLEMENTATION_PLAN.md).

See [verification and next steps](../../learning-plane/docs/STATUS.md) for actual build/runtime evidence.
