# ADR 0004 — Docker network topology: `velion-net` is the cross-plane shared bus; plane-specific nets are intra-plane only

- **Status**: proposed (ratify when team reviews)
- **Date**: 2026-05-11
- **Closes**: velion-gap.md G27 (decision); follow-up cleanup tracked separately
- **Supersedes**: none
- **Owners**: Frontend Plane (velion), Control Plane (auth/user/org/billing/session-core), Application Plane (convex, notification, integration), Data Plane v2, Model Plane, Ingestion Plane

---

## Context

`docker network ls` shows eleven networks. Six are meaningfully used:

| Network | Member count | Drivers / purpose (today) |
|---|---|---|
| `velion-net` | **48** | De-facto cross-cutting shared bus. Every running service in CoreSystem is attached (Control / Application / Data v2 / Model / Ingestion planes + velion + velion-nats). |
| `model-plane-network` | 18 | Intra-Model-Plane traffic. |
| `controlplane-net` | 15 | Intra-Control-Plane traffic. |
| `ingestion-net` | 15 | Intra-Ingestion-Plane traffic. |
| `dpv2-net` | 12 | Intra-Data-Plane-v2 traffic. |
| `app-net` | 8 | Intra-Application-Plane traffic. |

Only **two** containers are velion-net-only: `frontend-plane-velion-frontend-1` and `velion-nats`. Every other container is also on a plane-specific net — i.e. each container attaches to *two* networks: its plane network + velion-net.

The five empty-or-decommissioned networks (`aquatiq-backend`, `data-net`, `internal`, `visma_service_v2_default`, `xero_service_v2_default`) are zero-container artifacts of previous topologies that compose left behind.

`velion-net` is on subnet `172.20.0.0/16` — 65 534 host capacity, ~10× current population.

### Why `velion-net` looks misnamed today

The name suggests "the velion-frontend's own network." Two contradictions:

1. **Membership** — 48 containers attached, 46 of which are not velion (Control/App/Data/Model/Ingestion planes' Postgres / Redis / NATS / cores).
2. **Traffic** — velion talks to ~25 hostnames from its `.env` (auth-core, user-core, org-core, billing-core, session-core, notification-core, convex-backend, convex-gateway, documents-service, retrieval-service, ai-core, quarry-control, …). Every one of those is intra-plane to *its* plane, not to velion's. velion happens to be the busiest cross-plane caller because it's the L5 ingress (per [ADR 0003](./0003-l5-boundary-policy.md)).

### What the gap actually is

Two distinct concerns hide behind G27's "the network is misnamed":

1. **Naming.** A new contributor reads `velion-net` and assumes it scopes velion's traffic. The name is wrong — it scopes nothing.
2. **Least-privilege.** Today every container on velion-net can reach every other container on velion-net. `model-plane-postgres` can resolve `controlplane-postgres`. There's no reason it would, but the path exists. A future supply-chain compromise of any single container has visibility into every plane's NATS/Postgres/Redis cluster.

These can be tackled together or separately. This ADR picks one approach and notes the other as a future tightening pass.

## Options considered

### Option A — Rename, document, keep the shared bus

Acknowledge that `velion-net` IS the cross-plane shared bus and rename it accordingly (e.g. `coresystem-shared-net` or `inter-plane-bus`). Document the contract:

- **Intra-plane traffic** (e.g. `org-core` ↔ `controlplane-postgres`, `convex-backend` ↔ `app-nats`) must use the plane-specific network. Containers are physically attached to the shared bus too, but that bus is reserved for **cross-plane** edges.
- **Cross-plane traffic** (e.g. velion ↔ user-core, notification-core ↔ velion-nats, model-plane → control-plane auth-core) goes over the shared bus by design.
- A future tightening pass (Option B materialized) would actually remove the multi-attach where no cross-plane edge exists.

**Pros**:
- Matches operational reality. Zero behaviour change.
- One commit: rename the network in every compose file. Bounded blast radius.
- The "least-privilege" concern doesn't get worse than today.
- Charter ([`docs/ARCHITECTURE_DIAGRAM.md`](../ARCHITECTURE_DIAGRAM.md)) gains a network-topology section that matches what people see in `docker network ls`.

**Cons**:
- Doesn't shrink the attack surface. A future compromise still gets cross-plane visibility.
- The rename touches every compose file (Control / Application / Data v2 / Model / Ingestion / Frontend) — about 6 files × 1–2 hunks each. Coordinated but mechanical.
- Existing operators who alias `docker exec velion-net` will need to update their scripts.

### Option B — Shrink `velion-net` to the actual frontend boundary

Make `velion-net` contain only `velion`, `velion-nats`, and the **direct L1–L5 services velion proxies to** (auth-core, user-core, org-core, billing-core, session-core, notification-core, convex-backend, documents-service, retrieval-service, quarry-control). Remove everything else from velion-net; intra-plane networks remain for those services.

Cross-plane edges that today flow over velion-net but aren't velion → plane (e.g. model-plane → auth-core for JWT validation, ingestion → controlplane for some imports) would need a new dedicated network (`auth-public-net`? `control-readonly-net`?) or each cross-plane consumer joins the corresponding plane's network with a documented exception.

**Pros**:
- True least-privilege. A compromised dpv2-postgres can no longer route packets to controlplane-postgres.
- The name `velion-net` becomes literally correct.
- Forces cross-plane edges to be explicit. New cross-plane callers can't "just attach to velion-net"; they have to declare the edge.

**Cons**:
- Discovers many hidden edges. Today's model-plane services that reach auth-core, the ingestion plane services that reach quarry-control across plane boundaries, etc. — each becomes a network-membership decision.
- Touches every compose file and likely some env defaults (services that resolve other services' names will need different DNS targets or new aliases).
- Risk of breaking running deployments during the cutover — a service that "happened to work" because both endpoints were on velion-net will silently fail.
- 1–2 days of focused work + a careful per-edge audit. Premature given that no security incident has motivated it.

### Option C — Hybrid: rename now (Option A), shrink later under an explicit forcing function

Land Option A immediately (rename + doc). Open a **separate** gap entry to track the future shrink with a forcing function (e.g. "when we adopt zero-trust networking, or when we add the first PII-classified service that cannot share a broadcast domain with model-plane sandboxes").

**Pros**:
- Honest about reality today, leaves the better long-term answer in front of a real signal.
- Cheap to revert: a future ADR can supersede this one and pick Option B.
- Avoids over-engineering the topology before the second team / second tenant exists.

**Cons**:
- Same security-posture downside as Option A in the interim.
- Risk of "interim" becoming "permanent" without the named forcing function.

## Decision

**Choose Option C (Option A now + forcing function recorded for Option B later).**

Rationale:

- The naming fix is cheap and removes a real source of new-contributor confusion. Doing it now is uncontroversial.
- The shrink (Option B) is the right long-term posture but trades a real day of work + cutover risk for a hypothetical future threat. Today's deployment is single-tenant, single-team, dev/staging-grade. Defer until a forcing function lands.
- Option C names the forcing function explicitly so "interim" can't drift into "permanent" — see § "Forcing function for Option B" below.

This decision is also conditional on two guardrails landing alongside the rename (cheap; lint-only):

1. **Plane-specific networks remain mandatory for intra-plane traffic.** Adding a new service to its plane MUST also add it to that plane's network (`controlplane-net`, `app-net`, etc.), even if the shared bus would also work. A docker-compose convention check / pre-commit grep can enforce this.
2. **Document the cross-plane contract** in the amended `ARCHITECTURE_DIAGRAM.md` so contributors know when joining the shared bus is appropriate and when it isn't.

## Consequences

**Wins**:
- The name `inter-plane-bus` (or whatever the team picks during the rename PR) immediately stops misleading new contributors. `docker network ls` reads like an architecture diagram, not a coincidence.
- The charter doc finally has a network-topology section grounded in observable reality.
- Future option-B work has a clean baseline to measure against (vs. today's "rename will conflate two changes").

**Costs**:
- One coordinated rename across six compose files: `Control Plane/`, `Application Plane/`, `Data Plane v2/`, `Model Plane/`, `Ingestion Plane/`, `Frontend Plane/velion/`. Each compose declares `velion-net` as `external: true` and points at the same `name:`; flipping all of them in lockstep + `docker network create` for the new name + `docker network rm velion-net` after the cutover.
- Any operator script grepping for `velion-net` updates to the new name. Today's `docker network inspect velion-net` becomes `docker network inspect inter-plane-bus`.
- We accept the least-privilege gap until the forcing function in § below triggers Option B.

**What we're giving up**:
- Immediate adoption of true network-level least-privilege between planes. Reasonable trade today; not reasonable when a second tenant or a PII-classified workload ships.

## Forcing function for Option B

Open the shrink work (a new ADR 0005, "Supersedes 0004") when **any one** of the following lands:

1. **Second tenant on the same docker host** — multi-tenant sharing requires per-tenant isolation no matter how loose the dev posture is.
2. **First workload classified under data-residency / PII obligations** that names "shared broadcast domain" as a compliance blocker (e.g. SOC2 expansion to L4 separation, GDPR DPIA flagging cross-domain reachability).
3. **External penetration test result** that identifies the shared bus as exploitable — the test result is the forcing function.
4. **Anyone gets paged for the lateral-movement risk.** First real incident with cross-plane lateral movement triggers Option B immediately.

If 12 months elapse with none of the above triggering, this ADR's interim status is reconsidered explicitly (the alternative being "we ratify Option A permanently and rename the gap").

## Charter amendment (concrete text)

The following block lands in [`docs/ARCHITECTURE_DIAGRAM.md`](../ARCHITECTURE_DIAGRAM.md) as a new section after `## Cross-Plane Contract Rules`:

> ### Network Topology
>
> Docker networks split into two roles:
>
> - **Plane-specific networks** — one per plane, scopes intra-plane traffic.
>   `controlplane-net`, `app-net`, `dpv2-net`, `ingestion-net`,
>   `model-plane-network`. Containers within a plane talk to each other
>   over these. A new service in plane X **must** attach to plane X's
>   network even if `inter-plane-bus` is also attached.
>
> - **`inter-plane-bus`** — the cross-cutting bus where cross-plane edges
>   live. Velion (L5 ingress per ADR 0003) reaches every plane's public
>   surface over this bus. notification-core's subscriber to
>   `app.session.*` events (G14) flows here. Auth-core JWKS reads from
>   model-plane services flow here. **Not** for intra-plane traffic.
>
> Renamed from `velion-net` on 2026-05-11 (ADR 0004) — the old name
> conflated "velion's network" with "the shared bus." A future ADR may
> shrink the bus to only the cross-plane edges; see ADR 0004 § "Forcing
> function for Option B."

## Implementation plan

Mechanical, single-PR rename. Order matters because containers attached to the old network must detach before the network is removed.

1. **Author the new network name.** Recommended: `inter-plane-bus`. Alternatives the team can pick during PR review: `coresystem-shared-net`, `xplane-net`. The rename PR settles this.
2. **Create the new network.** `docker network create --driver bridge --subnet 172.20.0.0/16 inter-plane-bus` on each docker host. (Subnet preserved so existing IP-pinned configs continue to work.)
3. **Update each compose file** to replace `velion-net: external: true / name: velion-net` with the new name. Files to touch:
   - `Frontend Plane/velion/docker-compose.yml`
   - `Control Plane/docker-compose.yml`
   - `Application Plane/docker-compose.yml`
   - `Data Plane v2/docker-compose.yml`
   - `Model Plane/docker-compose.yml`
   - `Ingestion Plane/docker-compose.yml`
4. **Cutover sequence** per plane (15 min total when scripted):
   - `docker compose down` the plane,
   - apply the compose change,
   - `docker compose up -d` against the new network,
   - smoke-check the plane's own healthchecks pass and the velion proxy still reaches that plane's services.
5. **After all planes are up on the new network,** `docker network rm velion-net`.
6. **Update operator runbooks** — any `docker network inspect velion-net` line in `scripts/`, READMEs, or shell history aliases.
7. **Amend `docs/ARCHITECTURE_DIAGRAM.md`** with the "Network Topology" block above. Cite ADR 0004 inline.
8. **Update `velion-gap.md`** §1 / §10 entries that mention velion-net (search-and-replace).

## Implementation notes

- **Why not just rename in-place?** Docker doesn't support renaming a network. Workflow is create-new → migrate-containers → delete-old, hence the coordinated cutover.
- **Why preserve the `/16` subnet?** Any service that pinned an upstream by IP (we don't believe there are any, but auditing all envs is its own task) continues to work without surprise.
- **What about the empty networks?** `data-net`, `internal`, `aquatiq-backend`, `visma_service_v2_default`, `xero_service_v2_default` — 0 containers each. They're stale compose artifacts. Add a follow-up cleanup gap to delete them post-rename so `docker network ls` is honest.
- **Healthcheck during cutover.** Each plane's compose already declares healthchecks; the cutover verifies them. The velion ↔ session-core ↔ notification-core chain (Wave 3 §8.17) is the most useful end-to-end probe — a successful POST to `/api/v1/sessions/refresh` confirms cross-plane is alive.

## References

- `velion-gap.md` G27 entry
- `docs/ARCHITECTURE_DIAGRAM.md` (to be amended per § "Charter amendment")
- [ADR 0002 — CP session-core repurpose](./0002-cp-session-core-repurpose.md) (Control Session aggregator's NATS topology relies on the shared bus)
- [ADR 0003 — L5 boundary policy](./0003-l5-boundary-policy.md) (velion's role as L5 ingress is what makes velion the busiest cross-plane caller)
- Docker network reference: <https://docs.docker.com/network/network-tutorial-standalone/>
