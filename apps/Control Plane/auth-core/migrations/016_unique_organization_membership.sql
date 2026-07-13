-- Better Auth membership is canonical authorization state. Refuse to add the
-- uniqueness constraint while ambiguous rows exist: an operator must inspect
-- and remediate duplicates instead of this migration guessing which role wins.
DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM member
    GROUP BY organization_id, user_id
    HAVING COUNT(*) > 1
  ) THEN
    RAISE EXCEPTION
      'duplicate canonical membership rows exist; remediate before applying 016';
  END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS member_organization_user_unique
  ON member (organization_id, user_id);
