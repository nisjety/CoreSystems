# Velion UI and Information Architecture Brief

## Product Baseline

Velion should feel familiar to users of Intercom, Gorgias, Zendesk, and Chatbase without becoming a copy of any one product.

- Intercom sets the visual tone: calm, light, premium, clear.
- Gorgias sets the inbox operating model: queue left, conversation center, customer context right.
- Zendesk sets the helpdesk, reporting, and deep settings structure.
- Chatbase sets the agent, training, testing, and knowledge-builder workflows.

This should remain consistent across routing, naming, page layout, and UI density.

## Product Areas

Primary navigation:

- Overview
- Inbox
- Contacts
- Knowledge
- Agents
- Automations
- Reports
- Helpdesk
- Velion Chat
- Profile
- Settings

Notes:

- Keep Helpdesk separate from Inbox.
- Present People as Contacts in the UI.
- Keep Planner and Workspace unchanged for now.
- Use Velion Chat for direct AI work outside the shared inbox workflow.

## Layout Rules

- Do not add page-level background colors to dashboard pages.
- Let the dashboard layout own the page environment.
- Inner pages should inherit the same canvas behavior as Overview Home.
- Favor card and panel composition over full-page background blocks.

## Typography and Surface Direction

- Use Overview Home as the reference for cards, modal finish, spacing, and restraint.
- Keep page titles clean and premium, closer to Intercom than to a startup marketing site.
- Use tighter, denser typography in operational pages like Inbox and Helpdesk.
- Prefer soft white surfaces, thin borders, and low-contrast shadows.
- Reserve brand blue for actions, active states, and emphasis.

## Route-by-Route Product Model

### Overview

Purpose:

- Give operators a calm summary of the day.
- Surface the work that needs attention first.

Views:

- Home: keep as-is.
- Dashboard: shift toward queue and performance monitoring.
- Tasks: show both tasks and tickets in a Zendesk-like operator view.

Dashboard content:

- Today's volume
- Unassigned conversations
- SLA risk
- AI resolution rate
- Top channels
- Recent issues

Interaction model:

- Summary-first
- Minimal filtering compared to Inbox
- Fast jump points into Inbox, Helpdesk, Reports, and Contacts

### Inbox

Purpose:

- Live operator workspace for conversations and queue triage.

Layout:

- Left pane: queue list, filters, saved views, channels, assignee states
- Center pane: conversation thread and composer
- Right pane: customer, company, order/account, assignments, linked history

Core views:

- Assigned to me
- Unassigned
- All
- Mentions
- AI handled
- Spam

UI rules:

- Dense but calm
- Sticky queue filters
- Internal notes clearly distinct from customer-visible messages
- Composer should feel intentional and premium, not generic chat UI

### Contacts

Purpose:

- Customer and account context outside the live inbox.

Layout:

- Profile header
- Relationship and ownership panel
- Activity history
- Linked conversations
- Company/account context

Core content:

- Contact details
- Organization/account
- Assignee
- Recent conversations
- Tags
- Lead state

### Knowledge

Purpose:

- Source-of-truth management for AI and support content.

Core modules:

- Documents
- Articles
- Sources
- API integrations
- Training
- Test search

Key UI elements:

- Sync status
- Indexing state
- Usage metrics
- Source health
- Retrieval/testing surface

Tone:

- Chatbase builder logic
- Intercom clarity

### Agents

Purpose:

- Configure how Velion agents behave and when they hand work to humans.

Core modules:

- Instructions
- Tone
- Knowledge sources
- Actions
- Handoff rules
- Channels
- Playground

Interaction model:

- Clear setup flow
- Dense configuration where needed
- Separate test/playground area from production settings

Tone:

- Chatbase for builder ergonomics
- Intercom for visual calmness

### Automations

Purpose:

- Manage operational rules and shortcuts.

Core modules:

- Rules
- Triggers
- Macros
- Routing
- Escalations

UI pattern:

- Rule list
- Detail editor
- Condition/action builder
- Activity log or preview where useful

### Reports

Purpose:

- Measure team, channel, and AI performance.

Core modules:

- Team performance
- Channel performance
- AI performance
- Resolution metrics
- Trends
- Breakdown views

Tone:

- More structured and analytic
- Slightly denser, Zendesk-influenced layout

### Helpdesk

Purpose:

- Ticketing and operational support management separate from the live inbox.

Core modules:

- Ticket management
- Tasks
- Statuses
- Ownership
- Escalations
- Macros
- SLA monitoring

Design note:

- This area should feel more Zendesk-structured than Inbox.
- Keep Intercom softness in surfaces and spacing.

### Velion Chat

Purpose:

- Direct AI workspace for drafting, exploration, and one-off interactions.

Design note:

- Do not make this the same thing as Inbox.
- It should feel adjacent to the product, not like the primary operator queue.

### Profile

Purpose:

- Personal user settings and preferences.

Core modules:

- Profile details
- Sign-in and security
- Linked accounts
- Personal notifications

### Settings

Purpose:

- Workspace administration and governance.

Core modules:

- Inboxes
- Channels
- Integrations
- Members
- Permissions
- Billing
- Security
- Privacy
- Advanced controls

Tone:

- Most structured area in the product
- High clarity over aesthetics

## Implementation Order

Recommended next slices:

1. Inbox page shell and split-pane workspace
2. Helpdesk ticketing/task shell
3. Contacts profile shell
4. Overview dashboard refactor
5. Agents and Knowledge builder refinement

## Non-Goals For This Phase

- Reworking Planner
- Reworking Workspace
- Rebuilding Overview Home
- Adding page-specific background systems
