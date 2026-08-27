# Control Plane config

## `plane-service-principals.json`

The **policy half** of the cross-plane service principal registry: which
audiences each principal may address, the scopes it holds per audience, its
per-audience data-retention posture, and its organization binding.

It contains **no credentials**, and must never contain any. The secret half is
one credential per principal, minted by `scripts/run-control-plane.sh` into this
machine's gitignored `.env.generated-secrets` and assembled together with this
file into `PLANE_SERVICE_PRINCIPALS_JSON`.

### Why the split exists

The registry used to live entirely inside the gitignored store. A fresh checkout
therefore produced **no principals at all**, every cross-plane lane was closed,
and nothing in the repository said what was missing or why — the failures showed
up much later as a 401 from org-core or a 403 from auth-core, several services
away from the cause. Policy is reviewable and belongs in git; credentials are
per-machine and do not.

### Adding or changing a principal

Edit this file, then re-run the plane runner. The generator:

- **never rotates an existing credential.** A credential already present in the
  registry is reused as-is, so a re-run cannot invalidate one the running fleet
  is currently presenting.
- mints a fresh credential only for a principal that has none.
- publishes the credential to the principal's `consumerEnv` when one is
  declared, so the validating end and the presenting end resolve **one value**.
  Independent minting on the two ends is the single most common cause of the
  cross-plane auth failures this repo has seen.
- **reports rather than silently repairs** a registry/consumer mismatch. If the
  two disagree, one end is presenting a credential the other will reject, and
  that is a drift bug to fix at its source.

`consumerEnv: null` means the consuming service resolves the credential from its
own plane's environment rather than from Control's store. Those principals are
still registered here, but the generator cannot publish their value — currently
`embedding-engine`, `graph-index` and `retrieval-engine` (whose Data Plane
containers each read a same-named, per-container variable) and `corpus-seeder`.

### Retention markers are a data-retention decision

`retentionByAudience` is not a toggle. A credential minted as `zdr` is refused by
session-core for durable writes, which is correct for content that must not
persist and wrong for records that must — `execution-core`'s `session-core`
entry is `persistent` precisely because approval receipts are durable by nature,
while its `quarry` entry stays `zdr`. Change these with whoever owns retention
policy, not to make an error go away.

### Requires `jq`

The generator assembles JSON with `jq` and **fails loudly** if it is absent
rather than skipping the step — a silently skipped assembly would leave every
cross-plane lane closed while the bring-up still looked successful.
