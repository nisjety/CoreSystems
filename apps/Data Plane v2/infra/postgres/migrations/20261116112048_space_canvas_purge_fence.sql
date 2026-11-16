-- WS19-S8: the Space-deletion fence for room canvases. A Space purge records (org_id, space_ref) here in the same
-- transaction that deletes the Space's canvases. A create for a fenced Space is refused, so a create decided before the
-- deletion and written after it cannot leave canvas text behind. Org erasure deletes the org's fences too.
--
-- Only identifiers and a time are stored, never content. Applies after 20261116102141_space_canvas.sql.

CREATE TABLE IF NOT EXISTS space_canvas_purges (
    org_id     TEXT NOT NULL,
    space_ref  TEXT NOT NULL,
    purged_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, space_ref),
    CONSTRAINT space_canvas_purges_ident_chk CHECK (
        char_length(btrim(org_id)) > 0
        AND char_length(btrim(space_ref)) > 0
    )
);

-- dataplane_app reads the fence on every create. Only the unscoped purge path (Space deletion, org erasure) writes or
-- deletes it, so the app role holds SELECT alone.
GRANT SELECT ON public.space_canvas_purges TO dataplane_app;

ALTER TABLE public.space_canvas_purges ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS space_canvas_purges_rls_isolation ON public.space_canvas_purges;
CREATE POLICY space_canvas_purges_rls_isolation ON public.space_canvas_purges
    USING (org_id = current_setting('app.current_org', true))
    WITH CHECK (org_id = current_setting('app.current_org', true));
