-- The source lock belongs to the schema owner. SELECT FOR SHARE requires UPDATE
-- authority in PostgreSQL; the runtime role must never acquire that on evidence.
CREATE FUNCTION catalogue_projection_source_guard() RETURNS trigger LANGUAGE plpgsql
SECURITY DEFINER SET search_path = pg_catalog, public, pg_temp AS $$
BEGIN
 PERFORM id FROM public.raw_documents WHERE id=NEW.raw_document_id AND redacted_at IS NULL FOR SHARE;
 IF NOT FOUND THEN RETURN NULL; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION catalogue_projection_source_guard() FROM PUBLIC;
CREATE TRIGGER catalogue_projection_source_guard BEFORE INSERT OR UPDATE ON catalogue_projection
 FOR EACH ROW EXECUTE FUNCTION catalogue_projection_source_guard();
