CREATE TABLE IF NOT EXISTS learning_classes (
  id text PRIMARY KEY DEFAULT gen_random_uuid()::text,
  org_id text NOT NULL CHECK (btrim(org_id) <> ''),
  name text NOT NULL CHECK (btrim(name) <> '' AND length(name) <= 120),
  created_by text NOT NULL CHECK (btrim(created_by) <> ''),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (id, org_id)
);

CREATE TABLE IF NOT EXISTS learning_class_members (
  org_id text NOT NULL CHECK (btrim(org_id) <> ''),
  class_id text NOT NULL,
  user_id text NOT NULL CHECK (btrim(user_id) <> ''),
  role text NOT NULL CHECK (role IN ('teacher', 'student')),
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (org_id, class_id, user_id),
  FOREIGN KEY (class_id, org_id) REFERENCES learning_classes(id, org_id)
);

CREATE INDEX IF NOT EXISTS learning_class_members_by_user
  ON learning_class_members(org_id, user_id, class_id) WHERE active;

CREATE TABLE IF NOT EXISTS learning_courses (
  id text PRIMARY KEY DEFAULT gen_random_uuid()::text,
  org_id text NOT NULL CHECK (btrim(org_id) <> ''),
  class_id text NOT NULL,
  created_by text NOT NULL CHECK (btrim(created_by) <> ''),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (id, org_id, class_id),
  FOREIGN KEY (class_id, org_id) REFERENCES learning_classes(id, org_id)
);

CREATE TABLE IF NOT EXISTS learning_course_revisions (
  course_id text NOT NULL,
  org_id text NOT NULL CHECK (btrim(org_id) <> ''),
  class_id text NOT NULL,
  revision integer NOT NULL CHECK (revision > 0),
  title text NOT NULL CHECK (btrim(title) <> '' AND length(title) <= 160),
  description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
  content jsonb NOT NULL CHECK (jsonb_typeof(content) = 'object'),
  authored_by text NOT NULL CHECK (btrim(authored_by) <> ''),
  state text NOT NULL DEFAULT 'draft' CHECK (state IN ('draft', 'published')),
  created_at timestamptz NOT NULL DEFAULT now(),
  published_at timestamptz,
  PRIMARY KEY (course_id, revision),
  UNIQUE (course_id, org_id, class_id, revision),
  FOREIGN KEY (course_id, org_id, class_id)
    REFERENCES learning_courses(id, org_id, class_id),
  CHECK ((state = 'draft' AND published_at IS NULL) OR (state = 'published' AND published_at IS NOT NULL))
);

CREATE OR REPLACE FUNCTION learning_reject_published_revision_change()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.state = 'published' THEN
    RAISE EXCEPTION 'published course revisions are immutable';
  END IF;
  IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS learning_published_revision_immutable ON learning_course_revisions;
CREATE TRIGGER learning_published_revision_immutable
  BEFORE UPDATE OR DELETE ON learning_course_revisions
  FOR EACH ROW EXECUTE FUNCTION learning_reject_published_revision_change();

CREATE TABLE IF NOT EXISTS learning_assignments (
  id text PRIMARY KEY DEFAULT gen_random_uuid()::text,
  org_id text NOT NULL CHECK (btrim(org_id) <> ''),
  class_id text NOT NULL,
  course_id text NOT NULL,
  course_revision integer NOT NULL,
  assigned_by text NOT NULL CHECK (btrim(assigned_by) <> ''),
  due_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (id, org_id, class_id),
  UNIQUE (org_id, class_id, course_id, course_revision),
  FOREIGN KEY (class_id, org_id) REFERENCES learning_classes(id, org_id),
  FOREIGN KEY (course_id, org_id, class_id)
    REFERENCES learning_courses(id, org_id, class_id),
  FOREIGN KEY (course_id, org_id, class_id, course_revision)
    REFERENCES learning_course_revisions(course_id, org_id, class_id, revision)
);

CREATE INDEX IF NOT EXISTS learning_assignments_by_class
  ON learning_assignments(org_id, class_id, created_at DESC);

DO $$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'learning_classes', 'learning_class_members', 'learning_courses',
    'learning_course_revisions', 'learning_assignments'
  ] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
    EXECUTE format('DROP POLICY IF EXISTS learning_tenant_scope ON %I', table_name);
    EXECUTE format(
      'CREATE POLICY learning_tenant_scope ON %I USING (org_id = current_setting(''app.current_org_id'', true)) WITH CHECK (org_id = current_setting(''app.current_org_id'', true))',
      table_name
    );
  END LOOP;
END
$$;

GRANT SELECT, INSERT, UPDATE ON learning_classes, learning_class_members,
  learning_courses, learning_course_revisions, learning_assignments TO learning_app;
ALTER DEFAULT PRIVILEGES FOR ROLE learning_owner IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE ON TABLES TO learning_app;
