# Lane C progress (Go browser-broker, INNH-10 MP-1 / MP-2, security-sensitive)

Branch `mp-w2w3`, worktree `mp-w2w3-350537`. Everything dark behind `BROWSER_BROKER_KEY_GRANTS_ENABLED` (default off).

## Slice C0: OwnerFilter fail-open fix (done first)

Built:
- `apps/Model Plane/go/services/browser-broker/internal/authz/authz.go`: `OwnerFilter` returned `""` (any owner in the org, the scoped-service filter) for every principal that was not `user`. Now: `user` -> its ActorID, `service` -> `""`, anything else -> `noOwner` (a NUL-prefixed value that matches no stored owner; the Postgres store answers NotFound for it without a query). `Authorize` switches on the type: `user`, `service` (scope check), every other type refused outright (it used to fall into the scoped-service branch).
- Tests: `internal/authz/owner_filter_test.go` (OwnerFilter table: 6 cases; Authorize refuses 5 unknown types x 6 methods even with every scope), `internal/server/owner_scope_test.go` (an unknown-type or key-less api_key principal that got past Authorize gets NotFound on Validate and Revoke of a grant in its org; the owner still sees it).

Tests run: `GOWORK=off go test -count=1 -v ./...` in `go/services/browser-broker`: PASS=308 FAIL=0 (all packages ok).
Flags: none (a pure tightening).
