const project = dataform.projectConfig.defaultDatabase;
const entities = ["accounts", "invoices", "payments", "organisations", "contacts", "credit_notes", "journals"];

for (const entity of entities) {
  const staging = { database: project, schema: "dw_staging_xero", name: entity };
  declare(staging);

  assert(`xero_${entity}_source_keys`)
    .schema("dw_bronze_xero_assertions")
    .query(ctx => `
      SELECT tenant_id, record_id
      FROM ${ctx.ref(staging)}
      GROUP BY 1, 2
      HAVING tenant_id IS NULL OR record_id IS NULL
        OR COUNTIF(synced_at IS NULL) > 0 OR COUNT(*) > 1
    `);

  publish(`xero_${entity}`, {
    type: "incremental",
    schema: "dw_bronze_xero",
    protected: true,
    description: `Append-only typed observations from dw_staging_xero.${entity}; not original Xero API payloads.`,
    bigquery: {
      partitionBy: "DATE(synced_at)",
      clusterBy: ["tenant_id", "record_id"],
    },
  }).query(ctx => `
    WITH observed AS (
      SELECT source.*, TO_HEX(SHA256(TO_JSON_STRING(source))) AS version_id
      FROM ${ctx.ref(staging)} AS source
      WHERE source.tenant_id IS NOT NULL
        AND source.record_id IS NOT NULL
        AND source.synced_at IS NOT NULL
    )
    SELECT observed.*
    FROM observed
    ${ctx.when(ctx.incremental(), `
      WHERE NOT EXISTS (
        SELECT 1 FROM ${ctx.self()} AS prior
        WHERE prior.tenant_id = observed.tenant_id
          AND prior.record_id = observed.record_id
          AND prior.version_id = observed.version_id
      )
    `)}
    QUALIFY ROW_NUMBER() OVER (
      PARTITION BY tenant_id, record_id, version_id
      ORDER BY synced_at DESC
    ) = 1
  `);

  assert(`xero_${entity}_observation_grain`)
    .schema("dw_bronze_xero_assertions")
    .query(ctx => `
      SELECT tenant_id, record_id, version_id
      FROM ${ctx.ref({ database: project, schema: "dw_bronze_xero", name: `xero_${entity}` })}
      GROUP BY 1, 2, 3
      HAVING tenant_id IS NULL OR record_id IS NULL OR version_id IS NULL OR COUNT(*) > 1
    `);
}
