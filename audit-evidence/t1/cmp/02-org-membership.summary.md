# T1 / Category 2: Org & membership (cross-source comparison)

Inputs: 95 reference records (buzz 21, grokbot 11, qm 22, slack 18, teams 23) and 30 Verevon baseline records. Output: 37 clusters, all 95 records assigned, 0 excluded. Provenance tags: SRC = source code, REA = shipped binaries, DOC = documentation, INF = inference. Clusters file: `02-org-membership.clusters.json`.

## Shared baseline (table stakes)
- **Roles, permission checks and role change** (C-007). Every source has graded roles. Buzz has the best mechanism: pure policy functions plus a last-owner guard.
- **Room visibility, directory and self-join** (C-010). Buzz, Teams and QM have it (Slack too, but no record for it here).
- **Guests and external users** (C-011). Teams, Slack, QM and Buzz have it. QM requires an expiry on every guest.
- **Switching between accounts and workspaces** (C-002). Slack, Teams, Buzz and GrokBot have it.
- **Admin console** (C-018). QM, Buzz and GrokBot have one in code. Slack and Teams have admin centers, but there is no record for them in this category.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Role change and last-owner guard (C-007) | Buzz (SRC) | Pure functions for granting, changing and leaving. The role write is transactional and TOCTOU-safe. |
| Discovery and self-join (C-010) | Buzz (SRC) | The open-room directory is fetched separately from the membership poll. QM adds a deny-on-unknown fallback. |
| Guest tiers (C-011) | Teams (DOC) | Three clear tiers: external access, guest, and B2B direct connect. |
| Shared cross-org channels (C-012) | Slack (DOC) | Slack Connect, where the owning org controls the channel. QM (SRC) restricts agents when externals are in the room. |
| Agent access and add policy (C-015) | GrokBot (SRC q5) | Per-agent access list: owner, manager or collaborator, granted to users, groups, the whole team or a link, with an optimistic-concurrency token. Buzz adds the owner_only / nobody / anyone add policy. |
| Teams inside an org (C-008) | Teams (DOC q5) | Teams contain channels and are synced to M365 groups. In QM (SRC), team resources join a turn only under a membership rule. |
| Policy administration (C-019) | Teams (DOC q5) | Precedence is direct assignment, then group, then the global default. Policies come in packages, and batch rollout has a status log. |
| Invitations (C-013) | Buzz (SRC) | Stateless HMAC codes with a TTL and max uses. Claiming is rate-limited and records acceptance of the terms. |
| Scope layering (C-022) | QM (SRC q5) | One resolve() call mounts org and team layers read-only and the room scope read-write. |
| Tenure windows (C-023) | QM (SRC) | A sequence-bounded participant window filters transcripts and approvals. |
| Room lifecycle and TTL rooms (C-026) | Buzz (SRC) | Archive and unarchive, plus a TTL deadline that each new event pushes forward. Teams adds a 30-day restore window. |
| Sidebar sections (C-031) | Teams (REA q5) | Favorites, Muted, custom sections, drag reordering, and a rate-limited clean-up of stale channels. |

## Differentiators worth carrying
- Agent add policy and access lists (C-015). This is needed for personal agents.
- Room templates that come with an agent roster (C-027, Buzz). Neither Slack nor Teams has this.
- Publishing a personal agent to the team (C-016, GrokBot).
- Linking one person's identities across sign-ins (C-005, QM). QM's record is DOC only.
- Agent-led onboarding in the starter rooms (C-025).
- Setup manifests for agent computers at org, team or user level (C-036, GrokBot).

## Verevon's biggest gaps (priority tier)
1. **Room role change** (C-007, S). Control has viewer, editor, manager and owner, but there is no route or UI to change a role, and every grant is fixed to editor.
2. **Discovery, browse and self-join** (C-010, M). There are no routes. Control already stores a `privacy_revision`, which can be built on.
3. **Agent access and add policy** (C-015, M). Only the room side is gated today. Personal agents depend on this, and so does the rule that Space access never widens an ACL.
4. **Room lifecycle** (C-026, M). Shared rooms cannot be renamed, archived, deleted or restored. Only personal Spaces can be deleted.
5. **Admin UI for effect policies** (C-019, M). Every shared feature is denied by default, and there are no rows or admin tooling for these policies (backend baseline).
6. **Org room roster sync** (C-025, S). `syncAllOrganizationRooms` has no caller, so new members may be missing from #general (INF).

Recommended next, after the priorities above: teams and sub-groups, guests, link invites, SSO enforcement and SCIM, tenure windows, project rooms (currently a stub), sidebar sections, the non-member mention dialog, typed denial causes, and room templates.

Skip: client-held keypair identity (it conflicts with the Control authority model), communities (covered by rooms plus discovery), and sovereign clouds (a deployment concern, not a Space feature).

## Verevon strengths (keep)
- Signed, short-lived Control decisions bound to the authority, membership and audience revisions. They are equal to or stronger than QM's roster-version fencing (C-023).
- Effective access is the intersection of the Space decision, the audience and the owner-plane ACL. This is stricter than QM's rule that "every participant is entitled" (C-022).
- The room roster comes straight from Control. Agents are service subjects, and an agent that has access but no published identity is shown as such rather than hidden. A human editing the roster cannot remove agents (C-014, tier n/a).
- Policies are deny by default and carry compliance fields. ZDR fails closed, and the UI reports a gap as a gap rather than showing an empty view.
- Org-level custom roles over a capability catalog are richer than the closed role enums in Buzz and QM.

## Caveats
- The Slack and Teams records are mostly DOC or REA. QM's principal links (C-005) are DOC only, and GrokBot's team-agent records have medium confidence.
- Prevalence counts only the records in this category. Several "unique" clusters (SCIM, admin, sidebar) exist in products whose records fall in other categories, and the class labels were adjusted where noted.
- 16 clusters have no baseline record. Where I could, I set their status from my own spot-checks in code or from the baseline narratives: SSO, multiSession, invitations, billing flags, effect-policy migrations, the absence of @here, the absence of room templates, and setup manifests. The other 6 are `unknown`, each with a note on what to check (C-006, 029, 030, 034, 035, 036).
- buzz-product-034 is an INF "not built" record. I kept it as negative evidence in C-009 and did not count it as Buzz having the feature.
- Live behaviour is unverified, because dev has no `space_effect_policies` rows.
