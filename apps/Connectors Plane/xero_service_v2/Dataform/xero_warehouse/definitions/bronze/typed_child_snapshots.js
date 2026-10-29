const project = dataform.projectConfig.defaultDatabase;
const children = [
  { name: "invoice_lines", key: "line_item_id" },
  { name: "credit_note_lines", key: "line_item_id" },
  { name: "journal_lines", key: "journal_line_id" },
];

for (const child of children) {
  const source = { database: project, schema: "dw_staging_xero", name: child.name };
  declare(source);
  publish(`xero_${child.name}`, {
    type: "view",
    schema: "dw_bronze_xero",
    description: `Current typed snapshot of ${child.name}; staging has no child-level sync timestamp or version history.`,
  }).query(ctx => `
    SELECT source.*, TO_HEX(SHA256(TO_JSON_STRING(source))) AS version_id
    FROM ${ctx.ref(source)} AS source
  `);
  assert(`xero_${child.name}_source_keys`)
    .schema("dw_bronze_xero_assertions")
    .query(ctx => `
      SELECT tenant_id, ${child.key}
      FROM ${ctx.ref(source)}
      GROUP BY 1, 2
      HAVING tenant_id IS NULL OR ${child.key} IS NULL OR COUNT(*) > 1
    `);
}
