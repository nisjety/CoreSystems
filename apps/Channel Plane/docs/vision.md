# Channel Plane Vision

## Purpose

Channel Plane is the future runtime and deployment surface for external-facing Verevon agents.

It exists to let an organization create an agent in Verevon and deploy that agent into customer-facing channels such as:

- website chat widgets
- Shopify storefronts
- WooCommerce stores
- WordPress sites
- future channel adapters

This document captures the plan and vision for that future work.

## Current Decision

Verevon should continue focusing on getting the current platform running well before Channel Plane is built.

That means:

- the current architecture stays as-is for now
- no current plane needs to be restructured before launch
- Channel Plane is saved as a future expansion of Verevon, not a blocker for the current milestone

## Product Vision

Verevon serves two complementary use cases.

### 1. Internal Verevon Workspace

Employees use Verevon to:

- access organization knowledge
- ask questions over company documents
- work with internal agents
- perform day-to-day organization tasks

This remains powered by the existing platform foundations such as org, user, auth, and internal session handling.

### 2. External Agent Deployment

Organization admins can:

- create agents
- decide which internal users can monitor or manage those agents
- deploy agents into external channels
- monitor conversations in a shared inbox inside Verevon

This is the responsibility of Channel Plane.

## Architectural Role

Channel Plane should become the external conversation runtime for Verevon.

Verevon remains the parent platform and control surface.

Channel Plane becomes the operational layer for:

- widget bootstrap
- external visitor identity
- public conversation runtime
- channel adapters
- inbox and handoff operations
- realtime chat state

## Proposed Scope

### adapter-core

Owns platform-specific deployment and installation logic for channels such as:

- Shopify
- WooCommerce
- WordPress
- generic website embeds

Responsibilities:

- install callbacks
- adapter lifecycle
- webhook entrypoints
- deployment hooks
- platform-specific configuration

### widget-core

Owns widget deployment metadata and visitor bootstrap.

Core fields:

- `widgetId`
- `orgId`
- `agentId`
- `allowedDomains`
- `theme`
- `settings`
- `visitorId`
- `visitorSessionId`
- `conversationId`

Responsibilities:

- widget validation
- allowed-domain validation
- anonymous visitor bootstrap
- signed customer identity bootstrap
- browser token or cookie issuance for external chat sessions

### conversation-core

Owns external conversation runtime behavior.

Responsibilities:

- validate widget
- validate domain
- create or load conversation
- call agent or reasoning plane
- persist canonical messages
- publish events to downstream systems
- support operator handoff and inbox workflows

### Convex Realtime Layer

Convex should be used as the live transport and collaboration layer, not the only system of record.

Responsibilities:

- stream tokens
- typing indicators
- presence
- operator handoff updates
- inbox live updates

### Postgres Canonical Storage

Postgres should remain the canonical persistence layer for compliance-sensitive conversation records.

Responsibilities:

- conversation history
- message retention policies
- audit support
- deletion workflows
- reporting indexes

For GDPR, the retention target can be 30 days, but that should be enforced by application-level deletion jobs and data lifecycle workflows, not assumed as a native database TTL feature.

## Control Plane Responsibilities

Control Plane should continue owning configuration and governance.

It should know about:

- widget configurations
- channel installations
- deployment records
- conversation indexes
- audit trail
- billing and quotas
- agent access control

It should not become the hot path for public visitor traffic.

## Permissions Model

An organization admin should be able to:

- create an agent
- assign internal users access to that agent
- choose which users have read access
- choose which users have write or manage access
- deploy the agent to supported channels

Example:

1. Admin creates chatbot Z.
2. Admin gives user X and user Y permission to view and manage chatbot Z.
3. The agent is deployed through a WordPress or WooCommerce adapter.
4. Customers use the chatbot on the organization's public site.
5. Internal users with access can open the inbox in Verevon and monitor chatbot Z conversations.

## Key Principle

Channel Plane should be built as a new capability on top of Verevon, not as a reason to rewrite the current architecture.

## Related Future Scope

- [Meeting Intelligence](./meeting-intelligence.md) — capturing Teams/Zoom/Meet
  meetings as a source type (transcribe + document + search), processed entirely
  on-prem/open-source so meeting content never leaves the company. Capture is a
  channel adapter; processing and storage reuse the existing planes.

That is the main advantage of this plan:

- Verevon can move forward now
- Channel Plane can be added later
- the existing platform remains useful and relevant
- the future chatbot product can grow from the current system instead of replacing it

## Implementation Strategy

### Phase 1

Do not build Channel Plane yet.

Focus on:

- getting Verevon stable
- getting current planes running correctly
- validating the internal product experience

### Phase 2

Create the initial `channel-plane` implementation as a focused future module with:

- `adapter-core`
- `widget-core`
- `conversation-core`
- Convex-backed realtime support

### Phase 3

Expand Channel Plane into a stronger product surface inside Verevon with:

- external inbox operations
- channel analytics
- broader adapter support
- deployable chatbot workflows

## Final Position

Channel Plane is a future-facing Verevon capability and can eventually become a chatbot-focused product surface inside the larger Verevon platform.

It is the right direction.

It is also intentionally deferred.

The current priority remains getting Verevon running well with the architecture that already exists.