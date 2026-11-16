-- Reverse 20261116102141_space_canvas.sql. Drops the room canvas tables and every version in them. Canvas text is not
-- recoverable after this runs.
DROP POLICY IF EXISTS space_canvas_versions_rls_isolation ON public.space_canvas_versions;
DROP POLICY IF EXISTS space_canvases_rls_isolation ON public.space_canvases;
DROP TABLE IF EXISTS public.space_canvas_versions;
DROP TABLE IF EXISTS public.space_canvases;
