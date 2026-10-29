CREATE TABLE IF NOT EXISTS conversation_inboxes (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    channel TEXT NOT NULL DEFAULT 'email',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS conversation_inboxes_org_channel_unique
    ON conversation_inboxes (org_id, channel);

CREATE TABLE IF NOT EXISTS conversation_contacts (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    phone TEXT NOT NULL DEFAULT '',
    external_ref TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS conversation_contacts_org_email_unique
    ON conversation_contacts (org_id, lower(email))
    WHERE email <> '';

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    inbox_id TEXT NOT NULL REFERENCES conversation_inboxes(id) ON DELETE RESTRICT,
    contact_id TEXT REFERENCES conversation_contacts(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    priority TEXT NOT NULL DEFAULT 'normal',
    assignee_user_id TEXT NOT NULL DEFAULT '',
    assignee_name TEXT NOT NULL DEFAULT '',
    channel TEXT NOT NULL DEFAULT 'email',
    provider TEXT NOT NULL DEFAULT '',
    provider_thread_id TEXT NOT NULL DEFAULT '',
    last_message_preview TEXT NOT NULL DEFAULT '',
    last_message_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS conversations_org_status_updated_idx
    ON conversations (org_id, status, updated_at DESC);
CREATE INDEX IF NOT EXISTS conversations_org_assignee_updated_idx
    ON conversations (org_id, assignee_user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS conversations_inbox_updated_idx
    ON conversations (inbox_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS conversation_participants (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS conversation_participants_conversation_idx
    ON conversation_participants (conversation_id);

CREATE TABLE IF NOT EXISTS conversation_messages (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    direction TEXT NOT NULL,
    sender_type TEXT NOT NULL,
    sender_name TEXT NOT NULL DEFAULT '',
    sender_email TEXT NOT NULL DEFAULT '',
    body_text TEXT NOT NULL DEFAULT '',
    body_html TEXT NOT NULL DEFAULT '',
    internal BOOLEAN NOT NULL DEFAULT FALSE,
    provider TEXT NOT NULL DEFAULT '',
    provider_message_id TEXT NOT NULL DEFAULT '',
    provider_event_id TEXT NOT NULL DEFAULT '',
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS conversation_messages_conversation_occurred_idx
    ON conversation_messages (conversation_id, occurred_at ASC);
CREATE UNIQUE INDEX IF NOT EXISTS conversation_messages_provider_message_unique
    ON conversation_messages (org_id, provider, provider_message_id)
    WHERE provider <> '' AND provider_message_id <> '';

CREATE TABLE IF NOT EXISTS conversation_attachments (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    message_id TEXT NOT NULL REFERENCES conversation_messages(id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL DEFAULT '',
    size_bytes BIGINT NOT NULL DEFAULT 0,
    storage_ref TEXT NOT NULL DEFAULT '',
    provider_ref TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS conversation_tags (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS conversation_tags_org_name_unique
    ON conversation_tags (org_id, lower(name));

CREATE TABLE IF NOT EXISTS conversation_tag_links (
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES conversation_tags(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (conversation_id, tag_id)
);

CREATE TABLE IF NOT EXISTS conversation_sla_states (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    org_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'none',
    first_response_due_at TIMESTAMPTZ,
    resolution_due_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS conversation_ai_actions (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'suggested',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_by TEXT NOT NULL DEFAULT 'model-plane',
    reviewed_by TEXT NOT NULL DEFAULT '',
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS conversation_ai_reviews (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    ai_action_id TEXT NOT NULL REFERENCES conversation_ai_actions(id) ON DELETE CASCADE,
    reviewer_user_id TEXT NOT NULL,
    decision TEXT NOT NULL,
    comment TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS conversation_channel_thread_refs (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    connection_id TEXT NOT NULL DEFAULT '',
    provider_thread_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS conversation_thread_refs_unique
    ON conversation_channel_thread_refs (org_id, provider, connection_id, provider_thread_id);

CREATE TABLE IF NOT EXISTS conversation_idempotency_keys (
    org_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    outcome_conversation_id TEXT NOT NULL,
    outcome_message_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, idempotency_key)
);

CREATE TABLE IF NOT EXISTS conversation_audit_events (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL DEFAULT '',
    actor_user_id TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS conversation_audit_org_created_idx
    ON conversation_audit_events (org_id, created_at DESC);

CREATE TABLE IF NOT EXISTS conversation_events (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL DEFAULT '',
    type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS conversation_events_unpublished_idx
    ON conversation_events (created_at ASC)
    WHERE published_at IS NULL;
