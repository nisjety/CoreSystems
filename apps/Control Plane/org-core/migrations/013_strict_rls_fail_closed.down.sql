-- Roll back only the strictness change by restoring the 009 compatibility
-- branch. This is intentionally provided for controlled rollback rehearsal;
-- production should prefer fixing an unscoped caller over applying this file.
DO $relaxed_rls$
DECLARE
    schema_name text := current_schema();
    t text;
    key_column text;
    org_tables text[][] := ARRAY[
        ['organizations',                          'id'],
        ['org_entitlements',                       'org_id'],
        ['org_quotas',                             'org_id'],
        ['org_billing',                            'org_id'],
        ['org_compliance',                         'org_id'],
        ['org_role_mappings',                      'org_id'],
        ['org_plan_history',                       'org_id'],
        ['organization_members',                   'org_id'],
        ['org_tenant_links',                       'org_id'],
        ['org_onboarding_states',                  'org_id'],
        ['organization_domains',                   'org_id']
    ];
    i int;
BEGIN
    FOR i IN 1 .. array_length(org_tables, 1) LOOP
        t := org_tables[i][1];
        key_column := org_tables[i][2];
        IF NOT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = schema_name AND table_name = t
        ) THEN
            CONTINUE;
        END IF;
        EXECUTE format('DROP POLICY IF EXISTS %I ON %I.%I', t || '_rls_isolation', schema_name, t);
        EXECUTE format(
            'CREATE POLICY %I ON %I.%I '
            || 'USING (%I = current_setting(''app.current_org'', true) '
            || 'OR COALESCE(current_setting(''app.current_org'', true), '''') = '''') '
            || 'WITH CHECK (%I = current_setting(''app.current_org'', true) '
            || 'OR COALESCE(current_setting(''app.current_org'', true), '''') = '''')',
            t || '_rls_isolation', schema_name, t, key_column, key_column
        );
    END LOOP;
END
$relaxed_rls$;
