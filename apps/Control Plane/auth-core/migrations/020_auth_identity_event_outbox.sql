-- Transactional identity lifecycle outbox.
-- Better Auth owns the user/account writes. AFTER triggers capture delivery
-- intent in the same database transaction so broker outages cannot lose it.

CREATE TABLE IF NOT EXISTS auth_identity_event_outbox (
  event_id TEXT PRIMARY KEY,
  event_type TEXT NOT NULL CHECK (event_type IN ('user_registered', 'provider_linked')),
  user_id TEXT NOT NULL,
  payload JSONB NOT NULL,
  published_at TIMESTAMPTZ,
  dead_lettered_at TIMESTAMPTZ,
  processing_at TIMESTAMPTZ,
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_auth_identity_event_outbox_pending
  ON auth_identity_event_outbox (created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

CREATE OR REPLACE FUNCTION enqueue_auth_user_registered_event()
RETURNS TRIGGER AS $$
BEGIN
  INSERT INTO auth_identity_event_outbox (event_id, event_type, user_id, payload)
  VALUES (
    'user:' || NEW.id || ':registered',
    'user_registered',
    NEW.id,
    jsonb_build_object(
      'userId', NEW.id,
      'email', lower(trim(NEW.email)),
      'name', NEW.name,
      'provider', 'email',
      'emailVerified', NEW.email_verified
    )
  )
  ON CONFLICT (event_id) DO NOTHING;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_auth_user_registered_outbox ON "user";
CREATE TRIGGER trg_auth_user_registered_outbox
AFTER INSERT ON "user"
FOR EACH ROW EXECUTE FUNCTION enqueue_auth_user_registered_event();

CREATE OR REPLACE FUNCTION enqueue_auth_provider_linked_event()
RETURNS TRIGGER AS $$
DECLARE
  auth_user "user"%ROWTYPE;
BEGIN
  IF lower(NEW.provider_id) IN ('credential', 'email-password', 'password') THEN
    RETURN NEW;
  END IF;

  SELECT * INTO STRICT auth_user FROM "user" WHERE id = NEW.user_id;

  -- OAuth signups insert the user before the account in the same transaction.
  -- Enrich the still-pending registration event with the real provider before
  -- either event can be claimed by the delivery worker.
  UPDATE auth_identity_event_outbox
  SET payload = jsonb_set(payload, '{provider}', to_jsonb(NEW.provider_id)),
      updated_at = NOW()
  WHERE user_id = NEW.user_id
    AND event_type = 'user_registered'
    AND published_at IS NULL
    AND dead_lettered_at IS NULL
    AND created_at = transaction_timestamp();

  INSERT INTO auth_identity_event_outbox (event_id, event_type, user_id, payload)
  VALUES (
    'account:' || NEW.id || ':provider_linked',
    'provider_linked',
    NEW.user_id,
    jsonb_strip_nulls(jsonb_build_object(
      'userId', NEW.user_id,
      'email', lower(trim(auth_user.email)),
      'name', auth_user.name,
      'provider', NEW.provider_id,
      'providerAccountId', NEW.account_id,
      'scopesGranted', CASE
        WHEN NEW.scope IS NULL OR trim(NEW.scope) = '' THEN NULL
        ELSE to_jsonb(regexp_split_to_array(trim(NEW.scope), E'\\s+'))
      END
    ))
  )
  ON CONFLICT (event_id) DO NOTHING;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_auth_provider_linked_outbox ON account;
CREATE TRIGGER trg_auth_provider_linked_outbox
AFTER INSERT ON account
FOR EACH ROW EXECUTE FUNCTION enqueue_auth_provider_linked_event();
