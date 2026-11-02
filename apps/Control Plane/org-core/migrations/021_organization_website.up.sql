-- The organisation's own website, as its administrators gave it at onboarding. Org-core holds the company's
-- facts, so this is the copy other planes read. The visibility registry keeps its own copy of the sites it
-- monitors, linked by organisation id; neither copy is derived from the other.
--
-- The column checks the shape of an http or https address with a host name and no user information. The
-- Go code checks the same rule before the write, so a bad value is refused with a reason, not a database error.
ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS website_url TEXT
    CHECK (website_url IS NULL OR (char_length(website_url) BETWEEN 1 AND 2048
      AND website_url ~* '^https?://[^/?#@[:space:]]+[^[:space:]]*$')),
  ADD COLUMN IF NOT EXISTS website_updated_at TIMESTAMPTZ;
