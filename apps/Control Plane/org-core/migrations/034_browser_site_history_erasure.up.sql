-- F-f02-16: a member's remembered agent-browser site choices end with the membership
-- (a removal, or a reconcile remove from the Auth projection) and with the member's own
-- erasure (user self-erasure). The choices are deleted from user_browser_site_prefs.
--
-- The member's history rows are redacted, not deleted. The history keeps that a choice
-- changed at a revision, without saying whose it was or which host it named: the subject
-- and the actor become the fixed tombstone 'erased' and the before and after sets are
-- emptied. Nothing else on the row moves.
--
-- The runtime role gets UPDATE on exactly those four columns. The history stays
-- append-only everywhere else: the trigger below admits one transition only, a
-- user_pref row that is not redacted yet, moved to the tombstone with its id, org,
-- change kind, revision and created_at unchanged. Every other UPDATE is refused, so the
-- append-only check of repository_postgres_test.go still holds. DELETE stays granted to
-- no one, as migration 024 left it.

GRANT UPDATE (subject_user_id, actor_id, before_set, after_set) ON org_browser_site_history TO org_core_app;

CREATE OR REPLACE FUNCTION org_browser_site_history_redact_only()
RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.change_kind = 'user_pref'
     AND OLD.subject_user_id IS DISTINCT FROM 'erased'
     AND NEW.subject_user_id = 'erased'
     AND NEW.actor_id = 'erased'
     AND NEW.before_set = '{}'::jsonb
     AND NEW.after_set = '{}'::jsonb
     AND NEW.id = OLD.id
     AND NEW.org_id = OLD.org_id
     AND NEW.change_kind = OLD.change_kind
     AND NEW.revision = OLD.revision
     AND NEW.created_at = OLD.created_at
  THEN
    RETURN NEW;
  END IF;
  RAISE EXCEPTION 'org_browser_site_history is append-only: only the erasure of a member redacts a choice row'
    USING ERRCODE = '42501';
END;
$$;

DROP TRIGGER IF EXISTS org_browser_site_history_erasure_only ON org_browser_site_history;
CREATE TRIGGER org_browser_site_history_erasure_only
  BEFORE UPDATE ON org_browser_site_history
  FOR EACH ROW EXECUTE FUNCTION org_browser_site_history_redact_only();
