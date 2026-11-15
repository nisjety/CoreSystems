<a id="F-t1-048"></a>
### F-t1-048 · Room visibility, directory and self-join (org-public or private), with a late-joiner history rule

*Verevon today: Absent · Effort: L · Plane: Control Plane (visibility, join, tenure), Model Plane (read filter), Frontend Plane (directory) · Wave: W2 · Confidence: High*

**What and why.** Give named rooms an open or private visibility, a paged directory of org-public rooms the caller is not in, and a self-join that grants exactly room membership with the default role. Record join and leave sequence per membership so history can be limited to a tenure window, and make membership-change delivery durable. Browse and join is how people find work in Slack and Teams; today a person sees only rooms someone added them to.

**Reference practice.** Discovery and self-join ship in 3 of 5 references (Buzz, Teams, QM); a tenure window ships in 1 of 5 (QM). Buzz lists open channels in a directory and self-adds membership only for open rooms, while private rooms return not-found to non-members [SRC]. QM stores a participant window (valid-from and valid-to sequence) on membership rows and filters transcript reads by it, denying when membership is unknown [SRC]. Slack's public and private channels rest on public docs only [DOC].

**Verevon today and approach.** registered_spaces has no visibility column, only a privacy_revision counter, and space_memberships carries no join sequence (apps/Control Plane/user-core/migrations/017_space_authority.up.sql:10-66). The gateway has no browse or join route (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:649-735), and Session Core applies only an audience-revision ceiling to shared-thread reads, never a floor (apps/Model Plane/rust/services/session-core/src/grpc.rs:5438-5465) [SRC]. membership_changed already goes out over JetStream; the non-durable part is the plain-subscribe consumer and the missing publisher outbox. Decide the membership writer first, because the Application declarative sync (ReplaceMemberships) would revoke a Control-native self-join at the next sync. Keep rooms private by default with org-public opt-in, default open rooms to full history, and split the item: visibility, directory and join (M), then tenure window and durable delivery (L).

**Dependencies and risks.** Needs the default join role from [F-t1-022](#F-t1-022); the strict tenure floor belongs with guests in [F-t1-053](#F-t1-053). Directory names must respect residency and Zero-Data-Retention (ZDR), and self-join must never widen an owner-plane ACL.

<a id="F-t1-049"></a>
### F-t1-049 · Searchable people-and-agent directory with an agent-readable resolver

*Verevon today: Partial · Effort: M · Plane: Control Plane (people), Application Plane (agents), Frontend Plane gateway (composition) · Wave: W2 · Confidence: High*

**What and why.** Add a directory search over org members and registered agents that returns only subjects the caller may see, expose it to the add-member picker and later DM pickers, and give agents a resolve tool that answers ok, ambiguous (with candidates) or none (with a reason). Without it people cannot find each other past a flat list, and agents cannot address the right human.

**Reference practice.** All 5 of 5 references have a searchable directory. Teams resolves org directory, saved contacts and external users in one picker, finds externals only by exact email to prevent enumeration, and ranks by recent interaction [REA]. QM adds resolver vocabulary for agents: ok, ambiguous, none, not-in-channel and unlinked login [SRC].

**Verevon today and approach.** org-core already serves a member search by name or email substring (apps/Control Plane/org-core/internal/http/handlers.go:955-1060), but the gateway never proxies it and the add-member picker lists every active member with no search box (apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceMemberControls.tsx:86-135). An org-wide agent registry exists in the Application Plane (apps/Application Plane/convex-core/convex/spaceAgents.ts:477-512), but nothing merges people and agents and no agent-callable resolver exists [SRC]. Slice 1 (S): expose the org-core search through a gateway route that forces the session org and filters to active members, because the raw handler also returns invited emails. Slice 2 (M): compose people and agents in the gateway and expose the resolver as an agent tool with presence-not-authority semantics, since the registry answers who exists, never who may be invoked.

**Dependencies and risks.** The agent half is Application-owned under ADR-0002, so Control must not own the combined directory. External and guest lookup waits for [F-t1-053](#F-t1-053); DM and group-room pickers have no consumer until those Space kinds exist.

<a id="F-t1-050"></a>
### F-t1-050 · Meeting intelligence with consent: long-form transcription and an authority-scoped recap saved as a Space document, under a deny-by-default per-meeting AI policy

*Verevon today: Partial · Effort: XL · Plane: Model Plane (ASR, recap), Data Plane v2 (transcript units), Control Plane (meeting AI policy) · Wave: W4 · Confidence: Medium*

**What and why.** Transcribe uploaded recordings, then live calls, with speaker labels and timestamps; store segments as knowledge units rather than room posts; and write a recap (decisions, action items, chapters, citations) back as an editable Space document. Gate all of it with a Control-owned per-meeting policy (AI off, transcript only or full; privacy mode; ephemeral or retained answers; locked fields) that is checked before any automatic speech recognition (ASR) or agent reads audio.

**Reference practice.** Transcript and recap ship in 3 of 5 references (Buzz, Slack, Teams); a per-meeting AI policy in 2 of 5 (Teams, Slack). Teams produces notes, tasks, chapters and a speaker timeline with citations back to the recording, derives recap visibility from transcript access, and offers visible AI on/off, a privacy mode that blocks all agents, and admin-locked options rendered as locked [REA]. Slack announces to all when AI notes start and defaults off with external guests [DOC]. Grok Bot adds a consent card for agent attendance [SRC].

**Verevon today and approach.** Only dictation-sized speech-to-text exists: a 25 MB cap and a ZDR gate in apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:3952-4060, a 60 s take in the frontend, and no diarisation code anywhere. There is no meeting object, recap document or AI policy, and Control import decisions require a personal Space (apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go:111-128) [SRC]. Resequence: first the shared-room import decision and policy effects in Control; then an upload flow with a long-form ASR job extending the existing transcribe chain (self-hosted for ZDR tenants); then the recap as a Data Plane document with action items as approval cards; last the Meetings tab and policy chip. Split into an ASR and recap MVP (L) plus the policy and authority work.

**Dependencies and risks.** Needs the meeting record [F-t1-029](#F-t1-029) and a manager role, since membership adds carry no role. Transcript units must carry the meeting audience and policy revision so retrieval cannot leak them; ZDR must fail closed until a speech deployment is attested.

<a id="F-t1-051"></a>
### F-t1-051 · Retention policies for rooms, files and agent sessions, plus shared-Space deletion on the existing saga

*Verevon today: Partial · Effort: XL · Plane: Control Plane (policy), Application Plane (sweeper, shared deletion), Model Plane (thread purge) · Wave: W3 · Confidence: High*

**What and why.** Add org and per-Space retention (keep, N days, ZDR) carried in Control's signed decision and enforced by per-plane sweepers that return the same honest receipts the deletion saga uses, then extend that saga from personal to shared Spaces with owner approval and write fencing. Retention is a gating enterprise question and a GDPR storage-limitation duty, and today there is nothing between keep-forever and ZDR.

**Reference practice.** Retention appears in 3 of 5 references (Slack, Teams, and Buzz in its docs); durable shared-space deletion in 1 of 5 (Buzz). Slack layers policy objects at org, workspace, channel and DM scope, most specific wins unless the org locks it, and a scheduled sweeper deletes across stores with audited policy changes [DOC]. Buzz runs deletion as request, frozen inventory digest, owner approval, write fence, resumable checkpointed executor and final receipt [SRC].

**Verevon today and approach.** Control signs retention_class, residency and deletion_scope into every decision as opaque strings (apps/Control Plane/user-core/internal/spaces/authority.go:144-176), but no plane has a time-based sweeper; the only running one is audit-core's global purge (apps/Control Plane/audit-core/cmd/server/main.go:150-185). The deletion saga with per-owner receipts and legal hold is personal-Space only, and Session Core delete_space_threads is bound to one owner principal (apps/Model Plane/rust/services/session-core/src/grpc.rs:6058-6130) [SRC]. Phase it: add a numeric TTL to space_effect_policies and one Session Core sweeper for threads and messages plus the existing memory expiry (L); generalise owner adapters to "purge before timestamp for this Space" under a scheduled-purge scope; shared-Space deletion last, with purge re-keyed from owner-plus-Space to Space id.

**Dependencies and risks.** Legal hold outranks any sweep and ZDR must never weaken. Application may only request purges, never delete another plane's data. Search and graph index purge and copies in notification payloads stay open; whether saga migrations are applied in staging was not verified.

<a id="F-t1-052"></a>
### F-t1-052 · Typed timeline entries: server-signed system messages, a card registry with a safe fallback, link previews through Quarry, and pointer embeds for artifacts

*Verevon today: Partial · Effort: L · Plane: Model Plane (entry schema), Application Plane (system rows), Ingestion Plane (preview), Frontend Plane (card registry) · Wave: W2 · Confidence: Medium*

**What and why.** Give entries a content type and payload rendered through a card registry with a neutral fallback for unknown kinds, let the server emit typed system notices (member added, agent bound or paused, rename) that clients cannot forge, snapshot link previews at send through Quarry, and embed artifacts as pointers rendered under the viewer's own grant. Rooms then show what changed without a separate tab and never copy artifact content into messages.

**Reference practice.** Typed content and system messages each ship in 2 of 5 references, link previews in 3 of 5, pointer embeds in 1 of 5. Teams has 87+ typed system notices including agent added or removed [REA]; Buzz signs notices with the relay key and snapshots previews sender-side so recipients never contact the site [SRC]; Grok Bot renders unknown and retired preview types as fallback cards [SRC]; Teams Loop embeds carry their own ACL and warn when shared to a wider audience [REA].

**Verevon today and approach.** AppendMessageRequest has role, content and an opaque metadata struct but no content_type (apps/Model Plane/proto/model_plane/v1/sessions.proto:287-298); the timeline normaliser drops every role except user and assistant (apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomTimeline.tsx:21-67); the activity grammar has no membership class; Quarry has an SSRF guard and OpenGraph extraction but no metadata-only fetch mode [SRC]. Do not append notices into Session Core threads: the room is a list of thread-posts and each append needs a per-human Control decision. Emit notices as room-level feed rows from an Application projection of Control events, reserve content_type or an interim metadata.kind for in-thread cards, keep unknown kinds as a fallback, and add a light preview mode to Quarry.

**Dependencies and risks.** Needs the append path [F-t1-001](#F-t1-001) and canvas and lists ([F-t1-015](#F-t1-015), [F-t1-058](#F-t1-058)) for embeds. ZDR rooms disable snapshots; request-access stubs must not leak titles.

<a id="F-t1-053"></a>
### F-t1-053 · Guests and link invitations with mandatory expiry

*Verevon today: Partial · Effort: XL · Plane: Control Plane (auth-core identity, user-core membership) · Wave: W4 · Confidence: High*

**What and why.** Introduce a guest principal with memberships only in explicitly granted rooms, no directory, roster or agent invocation by default, invited through room-scoped invitations that must expire. Add shareable links or codes with TTL, max uses, optional admin approval and stored terms acceptance. Today a customer or contractor cannot be pulled into one room at all.

**Reference practice.** Guests ship in 4 of 5 references; link invitations in 2 of 5. Teams tiers guests [REA]; QM makes guest expiry mandatory and removes memberships through the normal change path [SRC]; Buzz encodes org, room, role, expiry, max uses and a nonce in an HMAC-signed invite code and rate-limits claims to 10 per minute [SRC]; Slack distinguishes single- and multi-channel guests with an admin approval queue [DOC].

**Verevon today and approach.** Only org-level email invitations exist, through the Better Auth organization plugin with a 48 h default expiry and a rate-limited accept route (apps/Control Plane/auth-core/src/auth/auth.ts:942-1013). space_memberships allows only user and service subjects (apps/Control Plane/user-core/migrations/017_space_authority.up.sql:51-66), room grants require an active org member and are hard-coded to editor, and ReplaceMemberships revokes any managed member not in the declared list (apps/Control Plane/user-core/internal/spaces/repository.go:693-775) [SRC]. Gate the build on two decisions: the guest identity model (separate principal versus restricted org role) and the cross-org identity and privacy contract the UI research doc requires. Reuse the Better Auth invitation for the email path with a room target, give guests a subject type excluded from the declarative sync, and ship link invites as a later slice.

**Dependencies and risks.** Needs room roles [F-t1-022](#F-t1-022) and the tenure window from [F-t1-048](#F-t1-048); a guest must never become a plain org member or enter org-visible documents. Whether the Data Plane honours the room audience for org documents was not traced; billing seats were not verified.

<a id="F-t1-054"></a>
### F-t1-054 · Teams and sub-groups with source-tracked membership; mentionable groups

*Verevon today: Stub · Effort: L · Plane: Control Plane (auth-core team store, user-core expansion) · Wave: W4 · Confidence: High*

**What and why.** Add a team concept (owners, members, visibility) whose grant expands into room memberships tagged with their source, so leaving a team removes only team-derived grants. Later, team mentions expanded server-side at send. Teams are the organising unit of the Teams half of the vision and the prerequisite for SCIM group sync and group-based agent access.

**Reference practice.** Teams ship in 2 of 5 references (Teams, QM); mentionable groups in 2 of 5 (Teams, Slack). Teams links team, team_members and team_rooms so joining a team grants its standard rooms while private rooms keep their own roster, and the server expands a group mention at send with per-recipient permission checks [REA]. QM includes a team layer in a turn only when the actor is on the team and the room is configured for it [SRC].

**Verevon today and approach.** Dormant scaffolding exists: auth-core defines team and team_member tables and an invitation team_id that nothing reads (apps/Control Plane/auth-core/src/db/schema.ts:225-262), and resource_grants reserves a team subject in its CHECK while the ACL repository rejects non-user subjects (apps/Control Plane/user-core/internal/users/acl_repository.go:58-85). Memberships carry only a free-text granted_by that the Application full-replace sync overwrites (apps/Control Plane/user-core/internal/spaces/repository.go:693-775), and no Space mention delivery exists [SRC]. Start from the auth-core tables by enabling the Better Auth teams option, since Auth Core is the membership authority and org-core deliberately mounts no membership mutations. Decide the single writer of room membership first: carry a source per member in the Application declaration, or let Control own writes with a source column and scope the sync to source=manual. Use the reserved team subject for team-level sharing.

**Dependencies and risks.** Needs [F-t1-022](#F-t1-022) and [F-t1-048](#F-t1-048); group mentions wait for Space notifications. Team membership never widens an owner-plane ACL, and agents in a mentionable group must be opt-in because one broadcast can trigger many paid turns.

<a id="F-t1-055"></a>
### F-t1-055 · Personal sidebar organisation: stars, custom sections, per-group sort, hide, mute, drag-and-drop, computed system sections and cross-org badges

*Verevon today: Partial · Effort: M · Plane: Application Plane (preferences), Frontend Plane (sidebar) · Wave: W2 · Confidence: High*

**What and why.** Give each user a synced sidebar preference document (stars, custom sections, per-group sort, hidden refs, mute) edited with drag-and-drop, plus computed system sections (Unread, Waiting for approval, Agent runs, Muted) over the same summaries, and later cross-org badges with keyboard switching. The need grows as soon as directory, DMs and team rooms add conversations per person.

**Reference practice.** Sidebar organisation ships in 4 of 5 references (Buzz, Grok Bot, QM, Slack); custom sections in 2 of 5 (Teams, Slack); computed system sections in 1 of 5 (Teams). Buzz stores a per-user blob of stars, sections, sort and hidden refs server-side, renders fixed groups then user sections, and drives a "More unread" pill from rows outside the viewport [SRC]. Teams computes Recent, Muted and Meeting-chats folders toggled in settings and offers a switch-back that keeps custom sections [REA]. Slack's workspace rail badges each tenant from its own summary [REA].

**Verevon today and approach.** The Spaces sidebar has fixed groups and a text filter with no stars, sections, hide, mute or room-level badge (apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/CoreSidebarSpacesPanel.tsx:30-232), and the WorkspaceSwitcher has no cross-org badge (apps/Frontend Plane/verevonv3/src/features/core/components/CoreNavbar.tsx:672-729). A reusable organiser already exists for chat threads, with native drag, an accessible Move menu and a zod-typed reducer, but it is browser-local (apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/ChatSidebarOrganizer.tsx:14-130) [SRC]. Unread is server-backed per room; the missing piece is a per-user, per-org attention summary. First slice: promote the chat organiser's pattern to a synced Convex preference document of refs only, then computed sections on the attention summary; cross-org badges and Ctrl+1..9 as a later item needing a multi-org read path.

**Dependencies and risks.** Hide, mute and sort are presentation only and must never change Space access; mute is a notification preference distinct from agent pause. Each gateway request resolves one org, so badges must never mix two orgs' data.

<a id="F-t1-056"></a>
### F-t1-056 · Agent task tracker: Needs-input state, per-task threads and unread in the Work tab

*Verevon today: Partial · Effort: L · Plane: Model Plane (task state), Application Plane (projection) · Wave: W3 · Confidence: Medium*

**What and why.** Give each agent task a visible state (working, needs input, done) with a note, link it to its run and Space thread, and project it with per-task unread into the Work tab and the activity inbox. A human should see exactly which agent task is waiting on them, which is the Grok Bot behaviour the vision asks for; today only runs awaiting approval surface.

**Reference practice.** Ships in 1 of 5 references (Grok Bot). Each task carries state working, needs_input or done, a note, done and seen-done timestamps and a sub-thread id; a message sent with a task id routes to the agent about that task, and a panel shows unread replies and done-but-unseen tasks [SRC].

**Verevon today and approach.** A durable task registry exists in capability-core with run_id, title, assignee and a free-text status that PATCH accepts (apps/Model Plane/go/services/capability-core/internal/api/workplane_apis.go:618-700), proxied by model-gateway /v1/tasks, but it is org-scoped with no space_ref, no Control decision on read or write, no agent producer and no caller in verevonv3. The Work tab lists runs and schedules with awaiting_approval as the only needs-a-human state (apps/Frontend Plane/verevonv3/src/features/spaces/lib/activity-grammar.ts:118-137), and no needs-input pause exists anywhere in the Model Plane [SRC]. Link tasks to runs and threads through run_id plus a Control-bound space_ref used only as a filter, following the Space cron pattern; add needs_input as a status value together with a Model-side ask-the-human pause; reuse the run's Space thread and the existing todos rather than inventing per-task threads. This is not a frontend-only change and is no longer a quick win.

**Dependencies and risks.** Keep agent-owned tasks separate from human Lists ([F-t1-058](#F-t1-058)) and link by ref; a free space_ref readable by any org caller would leak private rooms; ZDR runs must not persist task content.

<a id="F-t1-057"></a>
### F-t1-057 · Artifact sharing: edit role, Space-as-subject grants, general access and access requests

*Verevon today: Partial · Effort: L · Plane: Control Plane (user-core) · Wave: W2 · Confidence: High*

**What and why.** Extend the single resource_grants authority with an edit role exposed to the UI, a Space subject evaluated as the Space decision intersected with the grant, a general-access setting off by default per org, and an access-request flow with a human-only approve action, across documents, canvases and files. Without an edit role other members cannot co-author, and without access requests the decision never reaches an owner.

**Reference practice.** Ships in 4 of 5 references. QM offers a Google-Docs-style dialog where request access creates a pending item for the owner and the decide endpoint checks the decider manages the resource's home scope before granting, never downgrading a higher role [SRC]. Slack canvas grants are independent of conversation membership and mentions notify only people with access [DOC]; Grok Bot uses private, team-view and public tiers [SRC]; Teams delegates to SharePoint permissions [REA].

**Verevon today and approach.** Control already supports role edit and reserves a team subject (apps/Control Plane/user-core/migrations/012_resource_grants.up.sql:15-40); only the gateway hard-codes document, user and view (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/shares.rs:111-115). The real blocker is that POST and DELETE /authz/grant sit behind requireVerifiedAuthzDelegation, which admits only visible and check calls under v2 delegation while the gateway signs v1, so the share write path is fenced off today (apps/Control Plane/user-core/internal/http/server.go:384-400) [SRC]. First unblock that path with a verified resource-owner delegation or a dedicated grant-admin scope; then expose edit, add subject_type space with a CHECK migration, classify new resource types in resource_taxonomy.json with its Rust mirror, and build requests on the activity inbox. For documents reuse documents.space_ref instead of duplicating it with Space-subject grants.

**Dependencies and risks.** [F-t1-015](#F-t1-015) and [F-t1-013](#F-t1-013) cannot ship safely before phase 1. A Space subject may only narrow or co-require, never add an independent path; agents must never gain access through it; general access conflicts with ZDR and residency.

<a id="F-t1-058"></a>
### F-t1-058 · Typed room records and Lists: one revisioned work-object substrate with a table view, assignees and item threads

*Verevon today: Absent · Effort: L · Plane: Application Plane (records), Frontend Plane gateway (Control decisions) · Wave: W4 · Confidence: High*

**What and why.** Build a generic Space record primitive (stable id, namespaced type, body, previous-revision compare-and-swap, idempotent resubmit) and ship Lists on it: typed columns, a table view first, assignees and saved views. A list is where humans and agents assign work and see status, and one substrate avoids building tasks, decisions and checklists separately while keeping edits out of chat unread counts.

**Reference practice.** Lists ship in 3 of 5 references; a typed-record substrate in 1 of 5. Slack Lists define columns of text, select, people, date and checkbox, store views with filter, sort and group, and emit an activity item on assignment [DOC]. Buzz's record substrate keeps heads and revisions, advances the head by an atomic CAS on the previous revision, treats resubmits as idempotent, and excludes revisions from unread and notifications [SRC]. Teams offers Lists, Planner and Wiki tabs [REA].

**Verevon today and approach.** convex-core has no space_records tables (apps/Application Plane/convex-core/convex/schema.ts:546-604), the gateway has no records route (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:649-731) and the cockpit tabs are chat, work, knowledge, activity, agent and members. Adjacent scaffolding exists: workspaceId-keyed plannerDocuments with no consumer, thread-scoped Model Plane todos, org-scoped capability-core tasks and support tickets with saved views. Control resolves exactly one agent owner action today, tickets.create (apps/Control Plane/user-core/internal/spaces/model_action_view.go:12-26) [SRC]. Ship human-edited records first with gateway-side Control decisions for new read and write effects, because Convex does not evaluate Space roles. Reuse the Todo status vocabulary, register the table in the Space deletion flow, and take the agent write path out: it is a new owner-action vertical with schema hash, run-bound decision and approval continuation.

**Dependencies and risks.** Needs [F-t1-057](#F-t1-057) phase 1 and the activity inbox. Records must hang off spaceRef, never plannerDocuments or the Convex projects table (ADR rows 84-85); linked tickets and documents are refs rechecked by the owner plane; bundling the agent path makes this XL.
