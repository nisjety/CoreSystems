-- Per-user personal inbound-mail alias (chat-wave2 mail-task spec §3.1, owner
-- ruling 2026-09-28: Postmark inbound, self-forward only). An alias is an
-- opaque, high-entropy token (12 random bytes, base32 -- see
-- internal/mailtask/service.go) that maps 1:1 to a live (org_id, user_id): the
-- only address integration-corev2's inbound webhook receiver is ever allowed
-- to resolve into a chat thread.
--
-- Revoking never deletes or reuses a row -- RevokeAlias sets revoked_at and
-- mints a brand new row -- so a leaked or decommissioned alias can never
-- start resolving again by accident, and the resolve path can distinguish
-- "never existed" from "used to exist" (both collapse to the same HTTP
-- response, but only above this table -- see spec §6 alias-enumeration risk).
CREATE TABLE IF NOT EXISTS mail_task_addresses (
    alias      TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ
);

-- Exactly one LIVE alias per (org_id, user_id). GenerateAlias's idempotency
-- and RevokeAlias's revoke-then-mint both rely on this being enforced by the
-- database, not merely by application code racing against itself.
CREATE UNIQUE INDEX IF NOT EXISTS idx_mail_task_addresses_live_owner
    ON mail_task_addresses (org_id, user_id)
    WHERE revoked_at IS NULL;

-- The inbound webhook receiver's hot lookup is by alias alone (the primary
-- key already covers that); this index backs an admin/support "which alias
-- does this user have" reverse lookup without a sequential scan.
CREATE INDEX IF NOT EXISTS idx_mail_task_addresses_org_user
    ON mail_task_addresses (org_id, user_id);
