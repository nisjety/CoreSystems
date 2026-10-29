# Zammad Foundation

## 1. Executive Summary

This foundation keeps Zammad in charge of the parts it is already good at: mailbox ingestion, threading, follow-up detection, reply correlation, attachment handling, customer/ticket records, queues, agent workflow, and outbound replies through the correct mailbox identity.

The first version should stay close to stock Zammad. Do not pre-process inbound support email with AI before Zammad sees it. Let Zammad ingest from the support mailboxes directly, let Zammad decide whether a message is a new ticket or a follow-up, and only then let external services react through webhooks and API calls.

The resulting shape is:

- Zammad owns support channels, tickets, groups, SLAs, macros, and agent work.
- External services stay event-driven and post-ingestion only.
- Customization is limited to a small group model, a lean set of ticket fields, conservative triggers, and an integration-ready deployment boundary.

## 2. First-Version Zammad Architecture

### What Zammad should own now

- Direct mailbox/channel connections for support-facing inboxes
- New-ticket vs follow-up detection
- Message threading and reply correlation
- Attachment ingestion and storage
- Customer, organization, ticket, article, and owner records
- Agent queue/workspace
- Group-based outbound mailbox identity
- Basic triggers, tags, macros, and SLAs

### What should stay outside Zammad for now

- AI validation, spam/noise scoring, prioritization, and routing decisions
- Draft-reply generation
- Notification fan-out to Novu
- External connector sync through Nango
- Convex/NATS orchestration and event fan-out
- Branded customer UX and custom frontend

### What should be deferred

- Pre-ingestion message classification
- Non-email channel normalization layers
- Custom mailbox middleware
- Fully automated replies
- Multi-tenant portal UX
- Deep workflow packages or heavy theme changes

### Integration pattern to preserve

1. Zammad ingests the inbound message first.
2. A Zammad trigger fires a webhook after create/update.
3. External orchestration services inspect the ticket/article payload.
4. Those services update Zammad through the API, add notes, change fields, reassign groups, or ask humans to review.
5. Any future outbound reply should be created back in Zammad so ticket history and mailbox identity stay consistent.

## 3. Recommended Zammad Configuration

### Groups and queues

Use groups for internal work ownership and mailbox identity boundaries, not for AI classes.

- `Support`
  - Namespace only. Do not connect a mailbox directly here.
- `Support::Triage`
  - Default inbound queue for `support@` and `help@` when they share one outbound identity.
- `Support::Billing`
  - Separate queue if billing needs its own mailbox identity, SLA, or staffing.
- `Support::Escalations`
  - Manual escalation queue for issues that need senior handling.

Defer `Support::Sales` unless sales is genuinely part of this support runtime from day one.

### Roles

- `Support Admin`
  - Full Zammad admin rights for support operations only.
- `Support Lead`
  - `ticket.agent`, broad support group access, reporting/overviews, but limited configuration rights.
- `Support Agent`
  - `ticket.agent` plus full access only to the groups they work in.
- `Billing Agent`
  - Full access to `Support::Billing`; optional read access to `Support::Triage`.
- `Customer`
  - Stock customer role.

Avoid shared agent users. Use named human accounts only.

### Organizations

- One organization per customer/company when relevant.
- Keep `shared organization = no` by default.
- Use domain-based assignment only for trusted B2B customer domains.
- Do not model tenants as groups.

### Users

- Agents are humans.
- Mailboxes are channels, not agent users.
- Customers stay stock Zammad users.

### Tags

Keep tags small and operational.

- `channel:email`
- `mailbox:support`
- `mailbox:billing`
- `workflow:triage`
- `workflow:escalated`

Do not overload tags for AI state if custom fields already carry that information.

### Triggers

Start with conservative triggers only. Name them with numeric prefixes because Zammad evaluates triggers alphabetically.

- `00-tag-support-mailbox`
  - Condition: ticket created in `Support::Triage` from inbound email.
  - Action: add `channel:email`, `mailbox:support`, `workflow:triage`.
- `01-tag-billing-mailbox`
  - Condition: ticket created in `Support::Billing` from inbound email.
  - Action: add `channel:email`, `mailbox:billing`.
- `50-webhook-post-ingest-created`
  - Initially disabled until receiver exists.
  - Fires after new public email tickets are created in support groups.
- `51-webhook-post-ingest-updated`
  - Initially disabled until receiver exists.
  - Fires after new public customer articles are added to existing support tickets.

Do not add aggressive auto-close, auto-reply, or spam triggers in v1.

### Macros

Use macros for agent speed, not automation.

- `Assign to me + open`
- `Move to Billing`
- `Move to Escalations`
- `Need customer info`
- `Resolved - waiting for confirmation`

Macros do not send replies; use them for state/group/tag/note changes only.

### Automations and scheduler

Keep custom scheduler jobs empty in v1 unless you have a concrete need.

- Rely on native SLAs first.
- Add scheduler jobs only after you know what stale/backlog behavior you need.

### SLAs

Start simple.

- Calendar: business hours for your support timezone
- SLA `Support Standard`
  - Applies to `Support::Triage`
  - First response: 4 business hours
  - Update time: 8 business hours, `for an agent to respond`
  - Solution time: 3 business days
- SLA `Billing Standard`
  - Applies to `Support::Billing`
  - First response: 4 business hours
  - Update time: 8 business hours
  - Solution time: 2 business days

Use VIP organizations later if you need differentiated SLA policies.

### Channels and permissions

- Grant `admin.channel_email` only to support admins.
- Grant `admin.group`, `admin.object`, `admin.trigger`, `admin.webhook`, and `admin.sla` only to a tiny admin group.
- Normal agents should only get `ticket.agent` plus group access.

### Branding basics

Keep branding shallow in v1.

- Set instance name
- Set logo and favicon
- Configure notification sender
- Configure per-group signatures
- Keep deep theming and portal branding for the later custom frontend

## 4. Mailbox / Channel Strategy

### Core recommendation

Let Zammad own the support mailbox directly in v1.

That gives you:

- Native follow-up detection
- Native message-id and references handling
- Native attachments
- Correct reopen/new-ticket behavior
- Correct outbound identity per group

### Use multiple separate mailboxes only when identity or workflow differs

Recommended first setup:

- `support@company.tld`
  - Primary shared support mailbox
  - Connected directly to Zammad
  - Mapped to `Support::Triage`
- `help@company.tld`
  - Alias of `support@company.tld`
  - Only use aliasing when replies may come back from `support@company.tld`
- `billing@company.tld`
  - Separate shared mailbox
  - Connected directly to Zammad
  - Mapped to `Support::Billing`

Recommended deferral:

- `sales@company.tld`
  - Keep outside Zammad until you are sure sales belongs in the same runtime.
  - If you must include it, use a dedicated mailbox and group so replies come from `sales@company.tld`.

### Shared inboxes vs aliases

Use a real separate mailbox when:

- Outbound replies must come from that same address
- The team or SLA is different
- Reporting should treat it separately

Use an alias when:

- It is only another inbound name for the same team
- Outbound replies can use the primary shared address

### Mailbox-to-group mapping rule

Map mailboxes to groups based on the desired outbound sender identity.

- Group mailbox identity should be the system of record for replies.
- AI should recommend group changes after Zammad ingestion, not choose mailboxes before ingestion.

### Follow-up detection settings

Start conservative.

- Keep `Ticket Hook Position = right`
- Keep Zammad’s additional follow-up detection on `Subject & References`
- Do not enable body- or attachment-based matching until you prove you need it

If you later receive many forwarded `.eml` attachments and need those to land on existing tickets, enable attachment-based follow-up detection only after testing for false positives.

### Reply-To handling

Only change `Sender based on Reply-To header` if you knowingly ingest messages from forms or systems that rewrite sender identity into `Reply-To`.

### How to avoid pulling normal or personal email into Zammad

- Connect only shared support-facing mailboxes
- Never connect personal agent mailboxes
- Never auto-forward broad team mailboxes into Zammad unless they are support-only
- Prefer mailbox accounts or shared mailboxes created specifically for support operations

## 5. Ticket Model

### Native ticket model to use as-is

- `group`
- `owner`
- `customer`
- `organization`
- `priority`
- `state`
- `tags`
- ticket articles for the actual conversation

### Lean custom ticket fields

Create these as agent-facing internal fields:

- `request_type`
  - Select
  - Values: `general_support`, `billing`, `access`, `bug`, `feature_request`, `sales`, `spam_or_noise`, `other`
- `source_channel_key`
  - Select
  - Values: `email_support`, `email_billing`, `web_form`, `api`, `manual`, `other`
- `source_external_ref`
  - Text
  - For future external thread or connector IDs
- `ai_status`
  - Select
  - Values: `pending`, `skipped`, `triaged`, `approved`, `rejected`, `error`
- `ai_confidence`
  - Integer, 0-100
- `ai_category`
  - Text
- `ai_recommended_team`
  - Text
- `ai_review_required`
  - Boolean

### Field intent

- `request_type`
  - Human- or AI-assigned operational category
- `source_channel_key`
  - Stable routing/integration key
- `source_external_ref`
  - External correlation ID for future adapters
- `ai_*`
  - Post-ingest automation state only

### States

Keep native state types.

- `new`
- `open`
- `pending reminder`
- `pending close`
- `closed`

Do not add custom states until a real workflow demands them.

### Priority

Keep native priorities.

- `1 low`
- `2 normal`
- `3 high`

Let humans and later AI update priority; do not customize the priority model yet.

## 6. Future Extension Points

### Webhooks

Use Zammad webhooks as the event handoff after ingestion.

- Webhook target later: internal adapter on `verevon-net`
- Trigger source: ticket create and customer article update
- Payload consumer: Convex/NATS/AI orchestration service

### REST API

Use the Zammad API later to:

- read tickets and ticket articles
- update custom fields
- change group / owner / priority / state
- add internal notes
- create follow-up public articles when you intentionally want Zammad to send a reply

### NATS

Do not publish directly from Zammad. Let the webhook receiver publish a normalized event to NATS after validating the payload.

### Convex

Use Convex later as orchestration and operator state, not as the ticketing source of truth.

- store AI decisions
- store routing plans
- store review workflows
- fan updates back into Zammad through the API

### Novu

Do not wire Novu into Zammad yet.

Later:

- webhook receiver decides who needs notification
- Novu sends human notifications
- Zammad remains ticket state authority

### Nango

Use Nango later for third-party connector auth/sync.

- external systems ingest into your orchestration layer
- orchestration layer creates or updates Zammad via API when appropriate

### Custom frontend

Keep the custom frontend outside Zammad until later.

- Zammad remains the operational backend
- your frontend can eventually consume curated views or mirrored state
- do not let the frontend bypass Zammad’s mailbox/ticket logic

## 7. Step-by-Step Implementation Plan

1. Deploy the dedicated Zammad stack from [`docker-compose.zammad.yml`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/docker-compose.zammad.yml) using [` .env.zammad.example`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/.env.zammad.example) as your template.
2. On the Docker host, set `vm.max_map_count=262144` before starting Elasticsearch.
3. Start the stack with:
   - `cp .env.zammad.example .env.zammad`
   - `docker compose --env-file .env.zammad -f docker-compose.zammad.yml up -d`
4. Complete the Zammad first-run wizard or provide `AUTOWIZARD_JSON`.
5. In Zammad admin, set:
   - Ticket hook position to right
   - Additional follow-up detection to `Subject & References`
   - Notification sender
   - Sender format and separator
   - Maximum email size to a value matching your attachment expectations
6. Create the support calendar and the two starter SLAs.
7. Run the bootstrap script to create groups and ticket fields from [`bootstrap.example.json`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/config/bootstrap.example.json).
8. Execute object migrations and restart the Zammad app services.
9. Create the initial roles and assign group access.
10. Add shared mailbox channels directly in Zammad:
    - `support@`
    - `billing@`
    - optionally alias-backed `help@`
11. Map each mailbox identity to the correct group `Sending Email Address` and signature.
12. Add the safe starter triggers and macros manually.
13. Register, but initially disable, the future outbound triage webhook.
14. Run the checklist in [`admin-checklist.md`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/checklists/admin-checklist.md).
15. Run the validation scenarios in [`test-checklist.md`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/checklists/test-checklist.md).

## 8. Code / Config Scaffolding

### Runtime

- [`docker-compose.zammad.yml`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/docker-compose.zammad.yml)
  - Stock-lean Zammad runtime based on the official Docker Compose stack
  - Keeps internal dependencies inside `zammad-net`
  - Exposes app services to `verevon-net` for future internal webhook/API integration
- [`.env.zammad.example`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/.env.zammad.example)
  - Runtime variables and future webhook placeholders

### Bootstrap

- [`bootstrap/package.json`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/bootstrap/package.json)
- [`bootstrap/tsconfig.json`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/bootstrap/tsconfig.json)
- [`bootstrap/.env.example`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/bootstrap/.env.example)
- [`bootstrap/src/bootstrap.ts`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/bootstrap/src/bootstrap.ts)
  - Idempotent bootstrap for groups and ticket object attributes
- [`config/bootstrap.example.json`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/config/bootstrap.example.json)
  - Recommended starting group and ticket field definitions

### Integration planning

- [`config/webhook-plan.example.json`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/config/webhook-plan.example.json)
  - Future webhook and trigger blueprint for post-ingest orchestration

### Operations

- [`checklists/admin-checklist.md`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/checklists/admin-checklist.md)
- [`checklists/test-checklist.md`](/Volumes/Lagring/Triodelab/CoreSystem/apps/Application%20Plane/zammad-foundation/checklists/test-checklist.md)

## 9. Risks and Tradeoffs

### Keeping Zammad stock

Upside:

- low maintenance
- fewer upgrades to debug
- native mailbox behavior stays intact

Tradeoff:

- some setup remains manual in the admin UI

### Direct mailbox ownership by Zammad

Upside:

- best threading and reply correlation
- best update-vs-new behavior
- least custom code

Tradeoff:

- external AI only sees the message after ticket creation

### Separate mailboxes vs aliases

Upside of separate mailboxes:

- correct outbound identity by team

Tradeoff:

- more mailbox accounts to operate

### Self-hosted Docker vs Zammad Cloud

Upside of self-hosted:

- fits your Application Plane and future internal integrations
- easier to keep webhooks/internal adapters private

Tradeoff:

- more ops work than cloud

### Multi-tenant friendliness

Upside:

- organizations and group conventions give you a clean future path

Tradeoff:

- Zammad is not a full tenant-isolated platform by itself

## 10. Recommended MVP Setup

If you want the strongest first version with the least rework later, start with exactly this:

- Mailboxes:
  - `support@company.tld`
  - alias `help@company.tld` to the same mailbox
  - `billing@company.tld`
- Groups:
  - `Support::Triage`
  - `Support::Billing`
  - `Support::Escalations`
- Channels:
  - direct Zammad ownership of the real support mailboxes
- Fields:
  - only the eight internal fields in the bootstrap config
- Triggers:
  - mailbox-tagging only
  - future webhook triggers created but disabled
- SLAs:
  - one support SLA
  - one billing SLA
- Deferred:
  - sales mailbox
  - auto-replies
  - custom frontend
  - NATS/Convex/Novu wiring

## Sources

- Official Docker install docs: https://docs.zammad.org/en/latest/install/docker-compose.html
- Official Docker compose repo: https://github.com/zammad/zammad-docker-compose
- Official email settings docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/channels/email/settings.rst
- Official group settings docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/manage/groups/settings.rst
- Official webhook docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/manage/webhook.rst
- Official trigger docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/manage/trigger/how-do-they-work.rst
- Official macros docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/manage/macros/how-do-they-work.rst
- Official SLA docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/manage/slas.rst
- Official organization docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/manage/organizations/index.rst
- Official ticket settings docs: https://github.com/zammad/zammad-admin-documentation/blob/pre-release/settings/ticket.rst
- Official group API docs: https://docs.zammad.org/en/latest/api/group.html
- Official object API docs: https://docs.zammad.org/en/latest/api/object.html
- Official ticket API docs: https://docs.zammad.org/en/latest/api/ticket/index.html
- Official ticket article API docs: https://docs.zammad.org/en/latest/api/ticket/articles.html
