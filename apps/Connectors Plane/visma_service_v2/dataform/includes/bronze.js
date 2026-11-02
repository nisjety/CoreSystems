// Readers for one top-level field of a staging row's `payload` (a BigQuery JSON column), typed
// the way the v1 bronze tables typed their columns. Used by definitions/bronze/visma/*.sqlx.
//
// The payload is what Visma returned, untyped. A field that is missing or the wrong shape gives
// NULL, never an error: one odd row must not stop a whole table from building.

// `from` names the JSON column to read; the default is the staging row's own `payload`. A table
// that also takes fields from another endpoint passes e.g. "so.payload" (see OVERRIDES in
// tools/generate_bronze.py).
// "a.b" reads field b of object a: '$."a"."b"'. (No top-level payload key contains a dot.)
const path = (field) => `'$.${field.split(".").map((part) => `"${part}"`).join(".")}'`;
const raw = (field, from = "payload") => `JSON_VALUE(${from}, ${path(field)})`;

// Reads one scalar field and casts it: kind is "str" (text), "int", "float" or "bool".
const scalar = (kind, field, from) => module.exports[kind](field, from);

module.exports = {
  // Text. Objects and arrays come back as their JSON text (v1 stored nested values that way).
  str: (field, from = "payload") =>
    `IF(JSON_TYPE(JSON_QUERY(${from}, ${path(field)})) IN ('object', 'array'), ` +
    `TO_JSON_STRING(JSON_QUERY(${from}, ${path(field)})), ${raw(field, from)})`,

  // Whole numbers. A value written as 12.0 still counts.
  int: (field, from = "payload") =>
    `COALESCE(SAFE_CAST(${raw(field, from)} AS INT64), ` +
    `SAFE_CAST(SAFE_CAST(${raw(field, from)} AS FLOAT64) AS INT64))`,

  float: (field, from = "payload") => `SAFE_CAST(${raw(field, from)} AS FLOAT64)`,
  bool: (field, from = "payload") => `SAFE_CAST(${raw(field, from)} AS BOOL)`,
  ts: (field, from = "payload") => `SAFE_CAST(${raw(field, from)} AS TIMESTAMP)`,

  // A JSON object (as text) assembled from the fields of a nested object, under the key names
  // v1 used. `fields` maps an output key to a path inside `base`, or to [path, kind] to cast it.
  // Gives NULL when `base` is not an object. Example:
  //   object("branch", {number: "id", name: "description"})
  //   object("billing.address", {addressId: ["id", "int"], city: "city"})
  object: (base, fields, from = "payload") => {
    const entries = Object.entries(fields).map(([key, spec]) => {
      const [sub, kind = "str"] = Array.isArray(spec) ? spec : [spec];
      const value = kind === "str" ? raw(`${base}.${sub}`, from) : scalar(kind, `${base}.${sub}`, from);
      return `${value} AS \`${key}\``;
    });
    return (
      `IF(JSON_TYPE(JSON_QUERY(${from}, ${path(base)})) = 'object', ` +
      `TO_JSON_STRING(STRUCT(${entries.join(", ")})), NULL)`
    );
  },

  // Item attributes as one object {id: value}, the shape the Sales Order API (and so v1) had.
  // The ERP API returns them as a list [{id, value}].
  attributes: (from = "payload") => {
    const list = `JSON_QUERY_ARRAY(${from}, '$."attributes"')`;
    const keep = "WHERE JSON_VALUE(a, '$.id') IS NOT NULL";
    return (
      `IF(ARRAY_LENGTH(${list}) > 0, TO_JSON_STRING(SAFE.JSON_OBJECT(` +
      `ARRAY(SELECT JSON_VALUE(a, '$.id') FROM UNNEST(${list}) AS a WITH OFFSET AS i ${keep} ORDER BY i), ` +
      `ARRAY(SELECT IFNULL(JSON_VALUE(a, '$.value'), '') FROM UNNEST(${list}) AS a WITH OFFSET AS i ${keep} ORDER BY i))), NULL)`
    );
  },
};
