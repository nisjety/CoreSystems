-- P2-cp-04-rel1: the browser-site choices and history that a removal before migration 034
-- left behind. A one-time backfill, run once in the migration's own transaction.
--
-- A member removed before migration 034 kept the choices in user_browser_site_prefs and the
-- unredacted choice rows in org_browser_site_history. Migration 034 (the redaction grant and
-- trigger) and the removal path of P2-cp-04 erase from the removal on, and user self-erasure
-- fanned out only to active memberships, so nothing ever reached those rows. This applies
-- the same rule to the rows that exist now.
--
-- A member is active when organization_members says so, the predicate the resolver and the
-- removal path use. Every other member (removed, suspended or invited) is not active: a choice
-- of such a member is never applied, so deleting it changes no decision. A member who is made
-- active again later starts with no choices.
--
-- The history rows of those members are redacted to the shape that the redaction of migration
-- 034 produces: the subject and the actor become the 'erased' tombstone and the before and
-- after sets are emptied. That is the one transition 034's trigger admits, so it is unchanged.
-- Policy rows, whose actor is an administrator, are not touched. Rows already redacted are
-- skipped, so a repeat run changes nothing.

-- 1. The choices of members who are not active.
DELETE FROM user_browser_site_prefs p
WHERE NOT EXISTS (
  SELECT 1 FROM organization_members m
  WHERE m.org_id = p.org_id AND m.user_id = p.user_id AND m.status = 'active'
);

-- 2. The history of the choices of members who are not active.
UPDATE org_browser_site_history h
SET subject_user_id = 'erased', actor_id = 'erased', before_set = '{}'::jsonb, after_set = '{}'::jsonb
WHERE h.change_kind = 'user_pref'
  AND h.subject_user_id <> 'erased'
  AND NOT EXISTS (
    SELECT 1 FROM organization_members m
    WHERE m.org_id = h.org_id AND m.user_id = h.subject_user_id AND m.status = 'active'
  );
