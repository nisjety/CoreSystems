# zammad-foundation

## Current State

`zammad-foundation` is not part of the always-on main Application Plane runtime. It is a separate support-stack foundation package with its own compose file, bootstrap tooling, config examples, and operating guidance.

## Entry Points

- Support stack compose: `apps/Application Plane/docker-compose.zammad.yml`
- Foundation docs: `apps/Application Plane/zammad-foundation/README.md`
- Bootstrap package: `apps/Application Plane/zammad-foundation/bootstrap/`

## Relationships

- Adjacent to the Application Plane support domain.
- Referenced by frontend support and Zammad-related route helpers through environment-level Zammad URLs, not through a deep in-repo code dependency on the foundation package itself.
- Runs as a separate Zammad stack on `zammad-net` and `velion-net`.

## Stub, Mock, Placeholder, and Partial Audit

- The bootstrap package documents dry-run placeholder IDs, which is expected for bootstrap tooling.
- The foundation README also includes future webhook placeholder guidance.
- These are documentation and bootstrap placeholders, not evidence of a fake always-on runtime.

## Notes

Treat this package as a support-stack foundation and deployment aid, not as an always-on peer to `conversation-core-go` or `notification-core`.
