-- Distinguish source changes from projection writes so an idle worker does no work.
ALTER TABLE catalogue_generation ADD COLUMN source_generation bigint NOT NULL DEFAULT 0;
CREATE OR REPLACE FUNCTION catalogue_source_changed() RETURNS trigger LANGUAGE plpgsql
SECURITY DEFINER SET search_path = pg_catalog, public, pg_temp AS $$
BEGIN
 IF TG_TABLE_NAME = 'raw_documents' AND TG_OP = 'UPDATE' THEN
  DELETE FROM public.catalogue_projection WHERE raw_document_id = NEW.id;
 END IF;
 UPDATE public.catalogue_generation SET generation = generation + 1,
 source_generation = source_generation + CASE WHEN TG_TABLE_NAME = 'catalogue_projection' THEN 0 ELSE 1 END WHERE singleton;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION catalogue_source_changed() FROM PUBLIC;
DROP TRIGGER catalogue_projection_changed ON catalogue_projection;
CREATE TRIGGER catalogue_projection_changed AFTER INSERT OR UPDATE OR DELETE ON catalogue_projection
 FOR EACH ROW EXECUTE FUNCTION catalogue_source_changed();
