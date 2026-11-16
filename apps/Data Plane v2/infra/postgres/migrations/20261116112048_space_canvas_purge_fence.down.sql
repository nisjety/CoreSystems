-- Reverse 20261116112048_space_canvas_purge_fence.sql. Drops the Space purge fences. After this runs, a Space whose canvas
-- was purged can take a new canvas again, so run it only while no Space deletion is in flight.
DROP POLICY IF EXISTS space_canvas_purges_rls_isolation ON public.space_canvas_purges;
DROP TABLE IF EXISTS public.space_canvas_purges;
