# affine-core

`affine-core` is Aqencia's first-party planner workspace service in the Application Plane.

It owns:

- planner workspace bindings
- organization-aware and personal workspace resolution
- first-party session exchange API
- Control Plane-backed actor resolution from browser session cookies
- application-level event publishing for planner workspace lifecycle

The upstream AFFiNE runtime remains an internal dependency and is not exposed as the product boundary.
