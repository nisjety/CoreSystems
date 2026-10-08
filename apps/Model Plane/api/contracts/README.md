# Model Plane shared contract goldens

Files here are read by more than one plane. Each plane tests itself against the same file with
its own real code, so a rule that two planes must agree on is written down once and a
disagreement fails a test instead of failing in production.

| File | Written by | What it pins |
| --- | --- | --- |
| `infer-statuses.json` | inference-core (generated, `MP_UPDATE_GOLDEN=1`) | every way an Infer, CreateEmbedding or modality call can fail on the wire: gRPC code, `x-mp-provider-error` kind, retry timing |
| `zdr-posture-matrix.json` | authored (Z-10) | what a turn is (`zdr`, `persistent`, `waived`) or why it is refused, for the combinations of org posture, explicit ZDR, connection, capability, acknowledgement and outage |
| `connection-zdr-capabilities.json` | authored (Z-10) | whether a user-owned model connection can give ZDR, on what basis, and the digest that invalidates acknowledgements when that changes |

The two ZDR files are **authored specifications**, not captures of what a service emits. They are
held to a strict loader (`scripts/tests/zdr-contract-schema.mjs`) instead of a generator.

## How a golden is shared

A golden is never read from outside the tree of the code that consumes it (a test that skips when
its file is missing passes by not running). So:

- the canonical copy is here, with a `.sha256` pin beside it (sha256 of the file with CRLF
  normalised to LF);
- every consumer vendors a byte-identical copy **and** the pin inside its own tree;
- `scripts/tests/zdr-contract-sync.test.sh` is the repo-level drift check. It fails when a golden
  differs from the digest pinned in the script, when a pin file disagrees, when a vendored copy
  differs, when the strict loader rejects a ZDR golden, and, under `REQUIRE_CONTRACT`, when a
  consumer listed in its `CONSUMERS` table has not vendored yet. Without `REQUIRE_CONTRACT` a
  consumer that has not vendored is reported as `pending`.
- `REQUIRE_CONTRACT=1` belongs in any job that runs from a full checkout and expects the build to
  be complete (the Z-60 live proof gate; CI once every consumer has vendored). A consumer test
  that needs a file outside its tree must fail, not skip, when the variable is set.

Run it:

    bash scripts/tests/zdr-contract-sync.test.sh              # drift check
    bash scripts/tests/zdr-contract-sync.test.sh --self-test  # proves the check can fail
    REQUIRE_CONTRACT=1 bash scripts/tests/zdr-contract-sync.test.sh
    node --test scripts/tests/zdr-contract-schema.test.mjs    # the loader's own tests
    node scripts/tests/zdr-contract-schema.mjs                # validate the two ZDR goldens

## Changing a ZDR golden

1. Edit the JSON (LF, one trailing newline). Rows are data: add the row, never special-case a
   plane.
2. `node scripts/tests/zdr-contract-schema.mjs` and `node --test scripts/tests/zdr-contract-schema.test.mjs`
   must pass. Changing the capability of a provider, a basis or an auth mode needs `catalog_version`
   bumped and a digest vector for the new version, or the loader says so.
3. Refresh the pin beside the file: `sha256sum <file> > <file>.sha256` (keep the `<hash>  <name>` form).
4. Put the new digest in `GOLDENS` in `scripts/tests/zdr-contract-sync.test.sh`.
5. Re-vendor every copy listed in `VENDORED` (copy the file, copy the `.sha256`). The script names
   each stale copy.
6. Run every consumer's test. A plane whose real function disagrees with a row is the finding: fix
   the plane, or change the row deliberately, never both quietly.

## zdr-posture-matrix.json

Three tables, each evaluated by the real function of the plane that owns the rule:

- `org_posture_rows`: stored intent and plan entitlement to ZDR, not ZDR or unknown. Evaluated by every
  org-posture reader (Auth Core, the BFF, conversation-core).
- `decision_rows`: the retention decision for a turn. Evaluated by Auth Core's decision function
  (state, reason code, minted claims). The BFF checks `decision_call` and maps the answer;
  integration-corev2 checks the acknowledgement states of `vocabulary.acks`.
- `enforcement_rows`: what a plane that receives tokens does with them (bearers plus route).
  Evaluated by the shared `mp-contracts` resolver and, through it, model-gateway, inference-core,
  execution-core and session-core.

`vocabulary` closes every enum, `rule` states the order the decision applies, and `required_tags`
lists scenarios no edit may remove (plan lapsed, Temporary chat, Space-scoped, other connection,
other member, revoked acknowledgement, stale digest, text version below minimum, unknown provider,
each outage, non-owner, mixed ZDR token set, and the silent re-route the gateway used to do).

Every row has a `code`. A refusal carries a code of the Z-05a refusal catalogue; anything else
carries a reason code. The loader rejects unknown fields, missing fields, a code outside the
catalogue, duplicate rows, and any row that breaks a safety property: a waiver for anything but an
owner with a valid acknowledgement, a persistent answer for a ZDR org, a mismatched token set that is
served, a ZDR request served on a subscription route.

Two policies are pinned in `policy` and need an owner:

- `plan_lapse`: **intent_stays_zdr** is the applied default for OD-LAPSE (gate G-OD-LAPSE): an org
  whose plan lapses stays ZDR until an admin turns it off. Auth Core today computes
  `intent AND entitled` (`interactive-retention-policy.ts`, `isZdr`) and must change to match; the
  Control Plane wave owner has to be told. If the owner chooses the other reading, flip
  `policy.plan_lapse`, the `o_plan_lapsed_keeps_zdr` row and the two `d_plan_lapsed_*` rows
  together (the loader rejects a half flip, and its tests show the complete flip).
- `space_zdr`: **not_applied_in_v1** (honest-limits L14). The decision request carries no space id,
  so a Space with ZDR policy changes nothing in chat; `d_space_zdr_not_applied_org_off` pins that,
  so applying it later is a deliberate change of that row.

## connection-zdr-capabilities.json

One row per `(provider_key, auth_mode)` that has been assessed; anything else is the `fallback`
(`cannot_verify`, no waiver route). `cannot_verify` is a first-class state and is treated like
`not_supported` by the run rule, never rounded up. Only `supported` keeps ZDR on.

- integration-corev2 embeds the file (Go `go:embed`, vendored under its `internal/zdr/testdata/`),
  computes the digest (`digest.vectors` pin the canonical form) and owns the per-connection record.
- `cases` is the assessor's lookup rule: exact match, else fallback. The Go assessor must reproduce
  every case.
- `wording_keys` name the acknowledgement texts that must exist in `no` and `en` for every variant
  (`org_zdr_on`, `org_zdr_off`, `first_use`). The texts themselves (and the legal sign-off, OD-LEGAL)
  belong to integration-corev2.
- `tests/connection_zdr_capabilities_contract.rs` in inference-core reads the vendored copy and
  instantiates the real `CodexSubscriptionProvider`: a row is `supported` exactly when the provider
  reports `supports_zdr`, and a provider that is not `supported` refuses a ZDR request with
  `ZdrUnavailable` before it touches the network. Go cannot link Rust, so this check lives there.
  The same file loads the contract through a typed loader that denies unknown fields at every level
  and requires every scalar field (the Rust half of the loader rule), and re-hashes the digest
  vectors in Rust so the golden does not depend on one language's serialiser.
