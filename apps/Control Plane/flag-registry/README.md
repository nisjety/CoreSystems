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
9. `stage` is `dark`, `beta` or `ga`. An entry that leaves it out is `dark`. The stage is
   data: the registry does not roll anything out.
10. `agentAccess` is `none` or `read`. An entry that leaves it out gets `none`, so no
    model-facing tool may read the value.
11. `killSwitch` applies only to switches. A kill switch defaults to the safe value, which
    for a switch is off, and it is never `orgOverridable`. A switch that is on by default
    says why in `onDefaultReason` and does not carry `killSwitch`.
12. `orgOverridable` applies only to switches and choices owned by `frontend-plane` or
    `model-plane`. A per-org value may narrow the flag or enable it, but only within the
    build or environment ceiling. It never goes past that ceiling.
13. `onLiteral` is set only on a switch whose reader turns it on for one exact value: `true`,
    or `1` for a gate that reads 1. The purpose names that literal, and every declared
    default is off or that exact value, so `1` declared on a `true` switch fails the load.
    The typed read compares the raw value: any other value, padded or cased included, reads
    as off, as the service does. A purpose that says only the literal true turns it on must
    set `onLiteral` to match, and the word `literal` matches in any case, so `Literal true
    only` counts. `literal_switch_test.go` checks both directions.
14. `onLiteralAnyCase` is the one lenient switch reader the registry keeps. Its service trims
    surrounding space and folds ASCII case before it compares, so `TRUE` and ` true ` turn
    the switch on, and `1`, `on` and `yes` do not. Its value is always `true`, its purpose
    says `in any case`, and it never sits with `onLiteral`. The gateway's review run and
    teach mode use it, and the test pins those two.
15. `choiceRead` says how a choice's service compares the raw variable with `values`. Empty
    is trimmed: the service removes surrounding space and compares the rest exactly. `exact`
    compares the raw value, so a padded or cased value is refused. `anyCase` trims and folds
    ASCII case first, and its values are lowercase. `choice_read_test.go` pins the reader of
    every shipped choice, so a new choice cannot be added without its reader traced.

## Compose ratchet

The tests read `apps/Model Plane/deploy/docker-compose.yml` and
`apps/Control Plane/docker-compose.yml`, and check across both files:

- Every registered flag's declarations equal the compose declarations for that
  variable: service, container variable and default text, exactly. A variable set
  in both files is checked against the declarations of both.
- Every variable named by the convention (`_ENABLED`, `_MODE`, `_CONFIRMED`,
  `_SAMPLE_RATE`, `_ALLOW_*`) and set in compose is registered or listed in
  `unregistered`.
- Every name in `unregistered` is still set in compose.
- Every `codePath` still mentions its variable.
- Every switch the Control Plane compose sets is registered or listed in
  `controlSwitchUnregistered` with a reason. A switch is an interpolated name that follows
  the convention, or a value, interpolated or literal, that `ParseSwitch` reads. A numeric
  value that is not a switch is listed in `controlNotSwitches` with a reason.
  `control_compose_switch_test.go` holds both lists and pins the scan.
- Every Model Plane compose variable outside the convention whose compose default is a
  boolean-looking value (a non-blank word the canonical vocabulary reads, such as `0`,
  `false` or `off`) is registered or listed in `unregisteredSwitches`. A blank default is an
  unset value, so it is not counted. `catalogue_test.go` holds this second ratchet.
- A switch with an exact literal whose compose default is `0` names that literal in the
  comment above its line, so an operator who types `1` reads why it stays off.
  `model_compose_pins_test.go` pins this.

## Reader checks

- `switch_spellings_table_test.go` reads the exact-switch tables of the four Model Plane
  services (`switch_spellings.rs` in model-gateway, execution-core, session-core and
  inference-core). A switch a table reads by the exact literal must declare that literal as
  `onLiteral`, a choice a table reads exactly must declare `choiceRead: exact`, and the
  gateway's switches read true in any case must be exactly the rows that declare
  `onLiteralAnyCase`. A table row the test cannot read fails the test, so a changed shape
  is never skipped. A name with no registry row has no typed read, so it is not checked.
- `literal_reader_source_test.go` reads the Model Plane Go services and the Rust crates and
  services (their `src` trees), and finds every variable a reader takes by the exact literal
  `true` or `1`: `getenv`, `os.Getenv` or `lookup` compared with the literal; a Go helper that
  returns its parameter compared with it, called with an environment read; a Rust `switch_from`,
  or a function that reaches it or compares with the literal, called with an environment read; an
  environment read compared in its own statement; and an environment read bound to a local that a
  reader takes later in the same function. A Rust call resolves to the functions of its own file
  first, and a constant resolves through its module path, a `use ... as` rename, or the scope that
  declares it. Each such variable with a row must declare the literal as `onLiteral`. A reader whose
  variable the scan cannot name fails the test. The scan follows only these shapes, so the
  `literal_reader_source_test.go` shape test and its pins fail by name when one stops matching, and
  a value that a helper with several parameters carries is not followed.

The scanner reads a service at the indent of the first service under `services:`,
so the two-space Model Plane layout and the four-space Control Plane layout both
scan. `control_compose_test.go` pins the Control Plane names and their services.

`unregistered` is a visible baseline. It may only shrink. Registering a name means
adding its entry and removing it from the list.

## Adding or changing a flag

1. Add the entry in name order, or remove the name from `unregistered` first.
2. For compose flags, copy each `${HOST:-default}` line into `declarations`, with the
   service it sits in.
3. Run the tests. The ratchet fails until the entry matches compose. A new Control Plane
   switch needs an entry, or a reason in `controlSwitchUnregistered`, whether the compose
   writes it out or interpolates it.

Changing a compose default is a deployment change and goes through the owner and the
release process. The registry records the change; it does not make it.

## Not covered yet

- Model Plane compose variables outside the convention with a blank default, such as
  `MODEL_ACTION_ADMISSION`. A blank default is an unset value, not a switch word, so they
  are not ratcheted. Switch-shaped names outside the convention are listed in
  `unregisteredSwitches` until each gets an entry. Control Plane names are covered by the
  switch ratchet above.
- Literal compose values such as `MP_COMPAT_MODE: v1_only` in the Model Plane compose,
  which are not interpolated and are not ratcheted. In the Control Plane compose the
  literal switches are checked, and four stay out of the registry with a reason in
  `controlSwitchUnregistered`: `PPROF_ENABLED`, `DRAGONFLY_ENABLED`,
  `NATS_SHARED_ALLOW_TOKEN_FALLBACK` and `GF_USERS_ALLOW_SIGN_UP`. `GIN_MODE` is a
  framework value, not a switch.
- Compose files other than the two above: the Control Plane's
  `docker-compose.production.yml`, `docker-compose.legacy-bridge.yml`,
  `docker-compose.recovery.override.yml` and `docker-compose.shared-nats.yml`, and
  `apps/Model Plane/deploy/docker-compose.production.yml`.
- User and org settings (user-core `AISettings`, org entitlements). They need a typed
  value store before they can be registered.
- Compose files of the Data, Ingestion and Application planes. Frontend build-time
  `VITE_*` flags are registered only when an entry names the file that reads them
  (`source: code`, owner `frontend-plane`). No ratchet checks the frontend build or
  finds `VITE_*` names that are not registered.
- Per-org values for `orgOverridable` flags. The org-core store (W5-079-b) will hold
  them. Until then an org has no value of its own.
- `stage`, `killSwitch` and `agentAccess` are declared, not enforced. No service, BFF
  or SPA reads them yet.
- Typed reads are not wired into any service.
- A variable that two services read differently. `CONTROL_PLANE_ENFORCEMENT` is the example:
  the gateway trims and folds case, and retrieval-engine compares it exactly and reads any
  other value as `strict`, where the gateway reads it as `off`. The row models the reader
  named by its `codePath` (the gateway), so the other reader is not checked. An unset gateway
  reads the variable as `off`, while compose and the row default it to `strict`; the row
  cannot record that drift, because `codeDefault` applies only to compose flags.
- Choice readers that the registry models as `exact` but that also read a padded or cased
  off word as off: `false` for `TASK_EXECUTOR_ENABLED` and `off` for `CONTENT_WRITE_POLICY`.
  The registry refuses those spellings instead. The refusal is fail-closed, so a typed read
  never turns on what the service keeps off.

## Commands

```
cd "apps/Control Plane/flag-registry"
go test ./...
go vet ./...
```
