const documents = [
  { name: "expense_claims", endpoint: "expense_claims", description: "Xero expense-claim documents" },
  { name: "prepayments", endpoint: "prepayments", description: "Xero prepayment documents" },
  { name: "receipts", endpoint: "receipts", description: "Xero standalone receipt documents" },
];

for (const document of documents) {
  publish(`xero_${document.name}`, {
    type: "view",
    schema: "dw_bronze_xero",
    description: `Current ${document.description} derived from canonical immutable raw history. The source endpoint may validly be empty; this view never manufactures a typed staging table or infers deletion from absence.`,
  }).query(ctx => `
    SELECT generation, tenant_id, endpoint, record_id, version_id,
           observed_at, source_updated_at, run_id, receipt_id,
           source_status, source_deleted, payload_hash, payload_json
    FROM ${ctx.ref("dw_bronze_xero", "xero_raw_record_current")}
    WHERE endpoint = '${document.endpoint}'
  `);
}
