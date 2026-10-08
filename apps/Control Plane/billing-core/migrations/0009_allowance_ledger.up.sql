-- Allowance units are exact integers, independent of monetary account credits.
CREATE TABLE IF NOT EXISTS billing_allowance_pauses (
  org_id TEXT NOT NULL REFERENCES billing_accounts(org_id),
  started_at TIMESTAMPTZ NOT NULL,
  ended_at TIMESTAMPTZ,
  PRIMARY KEY(org_id,started_at),
  CHECK(ended_at IS NULL OR ended_at>=started_at)
);
CREATE UNIQUE INDEX IF NOT EXISTS billing_allowance_open_pause ON billing_allowance_pauses(org_id) WHERE ended_at IS NULL;
CREATE OR REPLACE FUNCTION billing_track_allowance_pause() RETURNS trigger AS $$
BEGIN
  IF NEW.subscription_state NOT IN ('active','trialing') THEN
    INSERT INTO billing_allowance_pauses(org_id,started_at)
    VALUES(NEW.org_id,clock_timestamp()) ON CONFLICT DO NOTHING;
  ELSE
    UPDATE billing_allowance_pauses SET ended_at=clock_timestamp()
    WHERE org_id=NEW.org_id AND ended_at IS NULL;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS billing_account_allowance_pause ON billing_accounts;
CREATE TRIGGER billing_account_allowance_pause AFTER INSERT OR UPDATE OF subscription_state ON billing_accounts
FOR EACH ROW EXECUTE FUNCTION billing_track_allowance_pause();
CREATE TABLE IF NOT EXISTS billing_allowance_policies (
  org_id TEXT NOT NULL REFERENCES billing_accounts(org_id),
  metric TEXT NOT NULL,
  policy JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (org_id, metric)
);
CREATE TABLE IF NOT EXISTS billing_allowance_entries (
  org_id TEXT NOT NULL,
  metric TEXT NOT NULL,
  entry_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('grant', 'usage')),
  units BIGINT NOT NULL CHECK (units > 0),
  occurred_at TIMESTAMPTZ NOT NULL,
  expires_at TIMESTAMPTZ,
  reason TEXT NOT NULL DEFAULT '',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (org_id, metric, entry_id),
  FOREIGN KEY (org_id, metric) REFERENCES billing_allowance_policies(org_id, metric),
  CHECK (expires_at IS NULL OR expires_at > occurred_at)
);
CREATE INDEX IF NOT EXISTS billing_allowance_entry_timeline ON billing_allowance_entries(org_id, metric, occurred_at, entry_id);
CREATE TABLE IF NOT EXISTS billing_allowance_snapshots (
  org_id TEXT NOT NULL,
  metric TEXT NOT NULL,
  snapshot JSONB NOT NULL,
  computed_at TIMESTAMPTZ NOT NULL,
  PRIMARY KEY (org_id, metric),
  FOREIGN KEY (org_id, metric) REFERENCES billing_allowance_policies(org_id, metric)
);
