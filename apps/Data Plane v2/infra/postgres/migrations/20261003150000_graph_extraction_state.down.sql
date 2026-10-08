-- Reverse 20261003150000_graph_extraction_state.sql.
--
-- The view goes before the table it reads; dropping the table takes its policy, trigger,
-- indexes, constraints and grants with it. The graph itself (graph_entities,
-- graph_relationships, graph_claims, graph_text_units, graph_communities) is not touched:
-- only what the migration recorded about each chunk (status, attempts, version stamps) is
-- lost. Re-applying the forward migration recreates the table and derives `ready` / legacy
-- rows again from graph_text_units; chunks that were `empty` are attempted once more.

DROP VIEW IF EXISTS v_graph_extraction_coverage;

DROP TABLE IF EXISTS public.graph_extraction_state;
