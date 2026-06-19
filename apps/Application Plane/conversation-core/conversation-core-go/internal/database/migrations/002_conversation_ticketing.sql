CREATE TABLE IF NOT EXISTS conversation_tickets (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ticket_key TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    priority TEXT NOT NULL DEFAULT 'normal',
    severity TEXT NOT NULL DEFAULT 'medium',
    category TEXT NOT NULL DEFAULT '',
    intent TEXT NOT NULL DEFAULT '',
    assignee_user_id TEXT NOT NULL DEFAULT '',
    assignee_name TEXT NOT NULL DEFAULT '',
    team_id TEXT NOT NULL DEFAULT '',
    team_name TEXT NOT NULL DEFAULT '',
    due_at TIMESTAMPTZ,
    source TEXT NOT NULL DEFAULT 'manual',
    ai_confidence DOUBLE PRECISION NOT NULL DEFAULT 0,
    ai_reason TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT '',
    waiting_since TIMESTAMPTZ,
    last_customer_reply_at TIMESTAMPTZ,
    first_response_at TIMESTAMPTZ,
    resolved_at TIMESTAMPTZ,
    snoozed_until TIMESTAMPTZ,
    sla_policy_id TEXT NOT NULL DEFAULT '',
    escalation_at TIMESTAMPTZ,
    labels TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, conversation_id),
    UNIQUE (org_id, ticket_key)
);

ALTER TABLE conversation_tickets
    ADD COLUMN IF NOT EXISTS waiting_since TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_customer_reply_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS first_response_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS snoozed_until TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS sla_policy_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS escalation_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS labels TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[];

CREATE INDEX IF NOT EXISTS conversation_tickets_org_status_updated_idx
    ON conversation_tickets (org_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS conversation_tickets_org_assignee_updated_idx
    ON conversation_tickets (org_id, assignee_user_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS conversation_tickets_org_due_idx
    ON conversation_tickets (org_id, due_at ASC NULLS LAST)
    WHERE due_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS conversation_tickets_org_sla_policy_idx
    ON conversation_tickets (org_id, sla_policy_id)
    WHERE sla_policy_id <> '';

CREATE INDEX IF NOT EXISTS conversation_tickets_org_escalation_idx
    ON conversation_tickets (org_id, escalation_at ASC NULLS LAST)
    WHERE escalation_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS conversation_tickets_org_labels_idx
    ON conversation_tickets USING GIN (labels);

CREATE TABLE IF NOT EXISTS conversation_ticket_views (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT 'org',
    owner_user_id TEXT NOT NULL DEFAULT '',
    team_id TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL DEFAULT 'sidebar',
    filter JSONB NOT NULL DEFAULT '{}'::jsonb,
    sort JSONB NOT NULL DEFAULT '{}'::jsonb,
    group_by TEXT NOT NULL DEFAULT '',
    sidebar_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(filter) = 'object'),
    CHECK (jsonb_typeof(sort) = 'object')
);

CREATE INDEX IF NOT EXISTS conversation_ticket_views_org_sidebar_idx
    ON conversation_ticket_views (org_id, visibility, sidebar_order, name);

CREATE TABLE IF NOT EXISTS conversation_ticket_macros (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL DEFAULT 'team',
    team_id TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    actions JSONB NOT NULL DEFAULT '{}'::jsonb,
    conditions JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(actions) = 'object'),
    CHECK (jsonb_typeof(conditions) = 'object')
);

CREATE INDEX IF NOT EXISTS conversation_ticket_macros_org_active_idx
    ON conversation_ticket_macros (org_id, active, name);

CREATE TABLE IF NOT EXISTS conversation_ticket_macro_runs (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    ticket_id TEXT NOT NULL REFERENCES conversation_tickets(id) ON DELETE CASCADE,
    macro_id TEXT NOT NULL REFERENCES conversation_ticket_macros(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL DEFAULT '',
    actions JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(actions) = 'object')
);

CREATE TABLE IF NOT EXISTS conversation_ticket_automation_rules (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    event_name TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    conditions JSONB NOT NULL DEFAULT '{}'::jsonb,
    actions JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(conditions) = 'object'),
    CHECK (jsonb_typeof(actions) = 'object')
);

CREATE INDEX IF NOT EXISTS conversation_ticket_automation_rules_org_event_idx
    ON conversation_ticket_automation_rules (org_id, event_name, active);

CREATE TABLE IF NOT EXISTS conversation_sla_policies (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    conditions JSONB NOT NULL DEFAULT '{}'::jsonb,
    calendar_ref TEXT NOT NULL DEFAULT '',
    first_response_minutes INTEGER NOT NULL DEFAULT 0,
    next_response_minutes INTEGER NOT NULL DEFAULT 0,
    resolution_minutes INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(conditions) = 'object')
);

CREATE INDEX IF NOT EXISTS conversation_sla_policies_org_active_idx
    ON conversation_sla_policies (org_id, active, name);

CREATE TABLE IF NOT EXISTS conversation_ticket_checklist_templates (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    items JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(items) = 'array')
);

CREATE TABLE IF NOT EXISTS conversation_ticket_checklists (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    ticket_id TEXT NOT NULL REFERENCES conversation_tickets(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    template_id TEXT NOT NULL DEFAULT '',
    created_by_user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS conversation_ticket_checklists_ticket_idx
    ON conversation_ticket_checklists (ticket_id, created_at DESC);

CREATE TABLE IF NOT EXISTS conversation_ticket_checklist_items (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    checklist_id TEXT NOT NULL REFERENCES conversation_ticket_checklists(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS conversation_ticket_checklist_items_checklist_idx
    ON conversation_ticket_checklist_items (checklist_id, position ASC, created_at ASC);

CREATE TABLE IF NOT EXISTS conversation_linked_resources (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    ticket_id TEXT NOT NULL REFERENCES conversation_tickets(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    link_type TEXT NOT NULL DEFAULT 'normal',
    resource_kind TEXT NOT NULL,
    resource_id TEXT NOT NULL DEFAULT '',
    resource_url TEXT NOT NULL DEFAULT '',
    label TEXT NOT NULL DEFAULT '',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_by_user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(metadata) = 'object')
);

ALTER TABLE conversation_linked_resources
    ADD COLUMN IF NOT EXISTS link_type TEXT NOT NULL DEFAULT 'normal';

CREATE INDEX IF NOT EXISTS conversation_linked_resources_ticket_idx
    ON conversation_linked_resources (ticket_id, created_at DESC);

CREATE INDEX IF NOT EXISTS conversation_linked_resources_resource_idx
    ON conversation_linked_resources (org_id, resource_kind, resource_id)
    WHERE resource_id <> '';
