-- WS17.E1 (plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS17.E1").
--
-- Envelope columns for profile snapshots. A row holds either the legacy
-- plaintext JSON in `snapshot` (modes off and compat), or an encrypted
-- envelope in `envelope` with `snapshot` NULL (mode on), or both during
-- the lazy re-seal window (`on` reads both and re-seals plaintext rows).
--
--   * envelope  — the EnvelopeV1 JSON bytes (ciphertext, wrapped DEK, nonce,
--                 snapshot id). Never plaintext cookies or storage.
--   * enc_v     — envelope format version (1 today).
--   * key_id    — the key that wrapped the DEK, `env:<master_id>` for the
--                 interim env provider; `oc:<n>` is reserved for WS17.E2.
--
-- Idempotent: every statement is guarded (IF NOT EXISTS). Applied files are
-- immutable; do not edit.

ALTER TABLE quarry_profiles
    ADD COLUMN IF NOT EXISTS envelope BYTEA    NULL,
    ADD COLUMN IF NOT EXISTS enc_v    SMALLINT NULL,
    ADD COLUMN IF NOT EXISTS key_id   TEXT     NULL;

CREATE INDEX IF NOT EXISTS quarry_profiles_key_id_idx
    ON quarry_profiles (key_id)
    WHERE key_id IS NOT NULL;
