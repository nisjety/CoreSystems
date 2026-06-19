# Recipe: add a Velion v3 SPA feature surface

> How to add a feature surface to the SolidJS SPA at
> `apps/Frontend Plane/velionv3/src`. References: `AiActionReviewPanel.tsx` (HITL),
> the Insights connector registry, the `/ingestions` Monitoring tab, and the
> AccountSettings `PrivacyDataSection`. NO Tailwind — style with semantic classes
> in `src/styles/global.css`; use `<For>`/`<Show>`, `props.x`, `createResource`,
> `createSignal`.

## The honesty checklist (read before you ship)

- [ ] **Every rendered number/label/status traces to a real gateway response**, or
      is an explicit empty / "not yet reporting" / "unavailable" / "attribution
      unavailable" state. No zeros-as-data, no placeholder metrics rendered as live.
- [ ] **No client org header.** Clients call same-origin `/api/v1/*`; the gateway
      resolves org/identity from the session. Never send `x-velion-org-id` (it is
      stripped at ingress) or an internal key. A client test should assert the
      header is absent.
- [ ] **Outcome copy matches reality.** "Decision recorded" (not "executed") when
      nothing consumes it yet; "On-demand only … recurring not yet available" when
      there is no scheduler; "Attribution unavailable" when you cannot prove a
      per-connection claim.
- [ ] **Irreversible/sensitive actions are gated** (typed-confirm + step-up
      re-auth) and only fire their side effect (e.g. sign-out) on a confirmed 2xx;
      the failure path stays put with an honest "did not complete — contact
      support" message. Test both paths.
- [ ] **Empty-state is an explicit `<Show>` fallback**, not a silent blank.

## Steps

1. **Client** in `src/shared/api/<feature>-client.ts`: thin `requestJson` wrappers,
   no custom org/auth headers. Type the gateway envelope; `requestJson` unwraps a
   top-level `{ data }`. Export any verbatim disclosure string as a constant and
   pin it with a test (see `privacy-client.ts` `CONTROL_PLANE_DSAR_DISCLOSURE`).

2. **Component** in `src/features/<area>/components/`: `createResource` keyed by the
   relevant id; render with `<Show when={!loading} fallback={…}>` and `<For>`; an
   explicit empty-state fallback; busy/disabled state on actions; honest error text.

3. **CSS**: add semantic classes (`.velion-<feature>-*`) to `src/styles/global.css`
   using the existing design tokens (`var(--border)`, `var(--muted-foreground)`,
   `var(--foreground)`, `var(--primary)`, `var(--destructive)`). No Tailwind.

4. **Test** (`*.test.ts(x)`, Vitest): assert the client sends no `x-velion-org-id` /
   no internal key, that envelopes parse, and any honesty invariant (e.g. a
   no-source event never becomes a per-connection claim).

## Quality gates

```bash
cd "apps/Frontend Plane/velionv3"
pnpm typecheck && pnpm lint && pnpm test && pnpm build
```

> Note: vitest fork workers can time out under heavy concurrent load (e.g. a Docker
> rebuild running at the same time), surfacing as "Failed to start forks worker"
> errors on unrelated files. Re-run `pnpm test` in isolation to distinguish a real
> failure from a resource-contention flake.

## Browser smoke

Log in (`local@velion.dev`), navigate to the surface, and confirm it renders **real
data or an honest empty-state** — never a fabricated value. For an action surface,
exercise it and verify the live result + that the copy matches what actually
happened.
