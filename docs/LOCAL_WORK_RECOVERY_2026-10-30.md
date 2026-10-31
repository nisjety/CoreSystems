# Local work recovery into Coresystem-verevon

Target repository: `https://github.com/nisjety/Coresystem-verevon.git`, branch
`main`. The remote was pulled at `121efe49` before conflict resolution.

The original backup remains at the sibling directory
`CoreSystem-backup-20261029-before-dev`. Recovery commit `56019468` reconstructs
the original tracked edits, deletions and non-private untracked files against
their original parent `da6fa0d4`. It does not import the unrelated CoreSystems
repository's root frontend or Application Plane stack.

The merge has both the existing main history and the recovery history as
parents. The original GitHub state is also retained locally on
`codex/verevon-main-before-recovery`.

## Resolution choices

- The user selected the backup's local marketing design. The separate
  `apps/Frontend Plane/verevonv3/apps/verevon-web` package uses that snapshot;
  the previous GitHub design remains available in the first-parent history.
- The Verevon product app retains GitHub's Solid 2 migration. Local Engagement,
  Visibility, Anbud, Studio demo and shell additions are adapted to its hooks,
  links, attributes and vendored icon renderer.
- Model subscription routes coexist with local Algolia, Clarity, analytics and
  organization website features. Provider configuration retains both sets of
  fields. Canonical Control Plane stream names retain the newly added consumers.
- The existing `/api/v1/onboarding/crawl-preview` streaming route remains.
  The restored bounded preview has its own authenticated pre-organization route,
  `/api/v1/onboarding/audit-preview`, used by the restored website preview UI.
- Quarry retains main's redirect validation and proxy-rotation logic while
  incorporating local bounded-body and single-hop discovery operations.
- The generated Quarry TypeScript client source and manifests are recovered
  from the backup as a workspace package. Build output and dependencies remain
  ignored. The typecheck command builds its SDK and remote-core dependencies.
- Private local environment files and credential settings remain excluded from
  Git. Dependency caches, build output and runtime state remain local artifacts.

## Verification scope

Validation covers the gateway, affected Go modules, frontend typechecks/builds,
restored frontend behavior tests and Quarry fetch operations. It does not claim
that every six-plane service has been started or every external integration has
been exercised against live providers.

The recovered marketing trust-claims suite references a local compliance
evidence bundle (`docs/compliance/evidence/claims-vs-code.md` and tools) absent
from both the supplied backup and main. That bundle is not fabricated as part
of this recovery; its evidence-dependent assertions require the real bundle.

Final checks for this merge:

- Gateway Rust tests: 583 passed.
- Quarry fetch tests: 20 passed.
- Affected Ingestion Go config/provider/API/action/Clarity/Algolia packages and
  audit-core provisioner tests: passed.
- Verevon product/feature/demo UI tests: 465 passed across 72 files.
- Marketing tests that have their fixtures: 122 non-React and 225 React tests
  passed. The evidence-bundle-dependent trust suite remains unverified.
- Verevon and marketing TypeScript checks: passed (the marketing production
  build also runs its TypeScript check).
- Product, standalone Studio demo and marketing production builds: passed.
- Gateway action/dispatch/security coverage contract tests: 7 passed.

The recovered feature modules use a narrow immediate-read signal adapter on top
of Solid 2's `latest` primitive for commands that write and then read within the
same operation. Their tests flush Solid 2's scheduler explicitly. No separate
Solid 1 runtime is introduced, and upstream feature modules keep their existing
Solid 2 semantics.
