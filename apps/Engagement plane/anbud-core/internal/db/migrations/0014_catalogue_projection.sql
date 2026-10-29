-- Rebuildable public catalogue metadata; source evidence remains append-only.
CREATE TABLE catalogue_projection (
 raw_document_id bigint PRIMARY KEY REFERENCES raw_documents(id),
 parser_version integer NOT NULL,
 payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object')
);
CREATE INDEX catalogue_projection_version ON catalogue_projection(parser_version, raw_document_id);
CREATE TABLE catalogue_generation (singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton), generation bigint NOT NULL DEFAULT 0);
INSERT INTO catalogue_generation(singleton) VALUES(true);

-- A redaction erases the derived copy in the same transaction. The application cannot
-- bypass this by retaining an old payload; readers join the live source as well.
CREATE FUNCTION catalogue_source_changed() RETURNS trigger LANGUAGE plpgsql
SECURITY DEFINER SET search_path = pg_catalog, public, pg_temp AS $$
BEGIN
 IF TG_TABLE_NAME = 'raw_documents' AND TG_OP = 'UPDATE' THEN
  DELETE FROM public.catalogue_projection WHERE raw_document_id = NEW.id;
 END IF;
 UPDATE public.catalogue_generation SET generation = generation + 1 WHERE singleton;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION catalogue_source_changed() FROM PUBLIC;
CREATE TRIGGER catalogue_raw_changed AFTER INSERT OR UPDATE ON raw_documents
 FOR EACH ROW EXECUTE FUNCTION catalogue_source_changed();
CREATE TRIGGER catalogue_search_changed AFTER INSERT ON raw_search_pages
 FOR EACH STATEMENT EXECUTE FUNCTION catalogue_source_changed();
CREATE TRIGGER catalogue_projection_changed AFTER INSERT OR UPDATE OR DELETE ON catalogue_projection
 FOR EACH STATEMENT EXECUTE FUNCTION catalogue_source_changed();
