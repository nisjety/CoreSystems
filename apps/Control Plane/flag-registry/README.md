# Flag registry

Control Plane's typed registry of feature flags and config knobs. Control owns the
registry; each entry names the plane that owns the flag. It absorbs the flags and
config ledger (`.plan-work/common/flags-and-config.md`) as typed data that tests can
check.

The registry declares. It does not read the environment for any service, and it
changes no behaviour. Services keep their own parsers until they migrate.
`ParseSwitch` is the canonical parser for new switches.

It stores names, defaults, owners and purposes. It holds no content, tokens or user
data, and it does not change ZDR behaviour. Flags that touch a content boundary keep
their posture in the owning service's matrix.

## Files

| File | Role |
| --- | --- |
| `flag-registry.json` | The registry. Schema `verevon.flag-registry/v1`. |
| `registry.go` | Types, the strict loader, the invariants, and the typed reads. |
| `parse.go` | The canonical switch vocabulary. |
| `compose.go` | The compose scanner and the naming convention. |
| `*_test.go` | Invariants, the parser, typed reads, the scanner, and the ratchets. |

## Entry rules

`New` rejects a document that breaks any of these. It reports every problem at once.

1. `name` is upper snake case and unique. `owner` is one of the closed set of planes.
   `purpose` is one trimmed line of at most 240 characters.
2. `source: compose` needs at least one declaration. `source: code` needs `codePath`, a
   relative forward-slash path that names the file reading the variable. `codeDefault`
   is for compose flags only.
3. Every declaration's default matches the flag default. Switch values compare by
   meaning, so a blank declaration and `0` both match an off default.
4. A switch parses in the canonical vocabulary. A switch that is on by default needs
   `onDefaultReason`. An off switch must not carry one. New risky behaviour defaults off.
5. If `codeDefault` differs from `default`, `driftReason` says why. Drift is recorded,
   not hidden.
6. `devOnly` marks a development escape hatch. It applies to switches and must default
   off.
7. A choice has unique `values` and a default among them. A text flag takes no values.
   Only switches take `onDefaultReason`, `driftReason`, `codeDefault` or `devOnly`.
8. `unregistered` is a sorted list of convention names with no entry yet.

## Compose ratchet

The tests read `apps/Model Plane/deploy/docker-compose.yml` and check:

- Every registered flag's declarations equal the compose declarations for that
  variable: service, container variable and default text, exactly.
- Every variable named by the convention (`_ENABLED`, `_MODE`, `_CONFIRMED`,
  `_SAMPLE_RATE`, `_ALLOW_*`) and set in compose is registered or listed in
  `unregistered`.
- Every name in `unregistered` is still set in compose.
- Every `codePath` still mentions its variable.

`unregistered` is a visible baseline. It may only shrink. Registering a name means
adding its entry and removing it from the list.

## Adding or changing a flag

1. Add the entry in name order, or remove the name from `unregistered` first.
2. For compose flags, copy each `${HOST:-default}` line into `declarations`, with the
   service it sits in.
3. Run the tests. The ratchet fails until the entry matches compose.

Changing a compose default is a deployment change and goes through the owner and the
release process. The registry records the change; it does not make it.

## Not covered yet

- Compose variables outside the convention (for example `DEEP_RESEARCH_PLAN_TURN`,
  `SPACE_WAKE_API`), which are often switch-shaped. They are not ratcheted.
- Literal compose values such as `MP_COMPAT_MODE: v1_only`, which are not interpolated.
- User and org settings (user-core `AISettings`, org entitlements). They need a typed
  value store before they can be registered.
- Compose files of the Data, Ingestion and Application planes, and frontend
  build-time `VITE_*` flags.
- Typed reads are not wired into any service.

## Commands

```
cd "apps/Control Plane/flag-registry"
go test ./...
go vet ./...
```
