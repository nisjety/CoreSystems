import { Pool } from "pg";
import type { RequestActor } from "@/lib/integrations/request-actor";

export type NavbarSearchResult = {
  id: string;
  label: string;
  excerpt: string;
  href: string;
  source: string;
};

type SearchableColumn = {
  table_schema: string;
  table_name: string;
  column_name: string;
};

type SearchScope = "all" | "knowledge";

const userColumns = ["user_id", "userId", "recipient_id", "owner_id", "created_by", "subscriber_id"];
const textTypes = new Set(["text", "character varying", "character", "jsonb", "json"]);
const knowledgeTablePattern = /(knowledge|document|source|page|node|chunk|embedding|collection|content)/i;

let pool: Pool | null = null;

function getPool() {
  if (!process.env.DATABASE_URL) {
    return null;
  }

  pool ??= new Pool({
    connectionString: process.env.DATABASE_URL,
    max: 4,
    idleTimeoutMillis: 30_000,
    connectionTimeoutMillis: 5_000,
  });

  return pool;
}

function quoteIdent(value: string) {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(value)) {
    throw new Error("invalid identifier");
  }
  return `"${value.replace(/"/g, "\"\"")}"`;
}

function routeForTable(table: string) {
  const normalized = table.toLowerCase();
  if (normalized.includes("conversation") || normalized.includes("message") || normalized.includes("chat")) {
    return "/chat";
  }
  if (normalized.includes("notification") || normalized.includes("inbox") || normalized.includes("ticket")) {
    return "/inbox";
  }
  if (normalized.includes("knowledge") || normalized.includes("document") || normalized.includes("source")) {
    return "/knowledge";
  }
  if (normalized.includes("agent")) {
    return "/agents";
  }
  return "/dashboard";
}

function toExcerpt(row: Record<string, unknown>, columns: string[], query: string) {
  const joined = columns
    .map((column) => row[column])
    .filter((value): value is string => typeof value === "string" && value.trim().length > 0)
    .join(" · ");
  const fallback = joined || query;
  return fallback.length > 180 ? `${fallback.slice(0, 177)}...` : fallback;
}

export async function searchSignedInUserDatabase(
  actor: RequestActor,
  query: string,
  options: { scope?: SearchScope } = {},
): Promise<NavbarSearchResult[]> {
  const db = getPool();
  const trimmed = query.trim();
  const scope = options.scope ?? "all";

  if (!db || trimmed.length < 2) {
    return [];
  }

  const [columnResult, allColumnResult] = await Promise.all([
    db.query<SearchableColumn>(
      `
        SELECT table_schema, table_name, column_name, data_type
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND data_type = ANY($1)
        ORDER BY table_name, ordinal_position
        LIMIT 240
      `,
      [Array.from(textTypes)],
    ),
    db.query<SearchableColumn>(
      `
        SELECT table_schema, table_name, column_name
        FROM information_schema.columns
        WHERE table_schema = 'public'
      `,
    ),
  ]);

  const grouped = columnResult.rows.reduce<Record<string, string[]>>((accumulator, row) => {
    const key = `${row.table_schema}.${row.table_name}`;
    return {
      ...accumulator,
      [key]: [...(accumulator[key] ?? []), row.column_name],
    };
  }, {});
  const columnSets = allColumnResult.rows.reduce<Map<string, Set<string>>>((accumulator, row) => {
    const key = `${row.table_schema}.${row.table_name}`;
    const current = accumulator.get(key) ?? new Set<string>();
    current.add(row.column_name);
    accumulator.set(key, current);
    return accumulator;
  }, new Map());

  const searchPattern = `%${trimmed.replace(/[%_]/g, "\\$&")}%`;

  const searchTasks = Object.entries(grouped).flatMap(([tableKey, columns]) => {
    const [schema, table] = tableKey.split(".");
    if (scope === "knowledge" && !knowledgeTablePattern.test(table ?? "")) {
      return [];
    }

    const allColumnNames = columnSets.get(tableKey) ?? new Set<string>();
    const availableUserColumn = userColumns.find((column) => allColumnNames.has(column));
    const allowedUserTable = table === "users" && allColumnNames.has("id");

    if (!availableUserColumn && !allowedUserTable) {
      return [];
    }

    const comparableColumns = columns.slice(0, 6);
    const whereText = comparableColumns
      .map((column) => `${quoteIdent(column)}::text ILIKE $1 ESCAPE '\\'`)
      .join(" OR ");
    const userFilter = allowedUserTable ? `${quoteIdent("id")} = $2` : `${quoteIdent(availableUserColumn ?? "")}::text = $2`;
    const idColumn = allColumnNames.has("id") ? "id" : comparableColumns[0];

    return [
      async () => {
        const rows = await db.query<Record<string, unknown>>(
          `
            SELECT ${quoteIdent(idColumn)}::text AS __id, ${comparableColumns.map((column) => quoteIdent(column)).join(", ")}
            FROM ${quoteIdent(schema)}.${quoteIdent(table)}
            WHERE (${whereText}) AND ${userFilter}
            LIMIT 12
          `,
          [searchPattern, actor.userId],
        ).catch(() => ({ rows: [] }));

        return rows.rows.map((row) => {
          const id = typeof row.__id === "string" ? row.__id : crypto.randomUUID();
          const excerpt = toExcerpt(row, comparableColumns, trimmed);

          return {
            id: `${table}-${id}`,
            label: table.replace(/_/g, " "),
            excerpt,
            href: `${routeForTable(table)}?q=${encodeURIComponent(trimmed)}`,
            source: table,
          } satisfies NavbarSearchResult;
        });
      },
    ];
  });

  const results = (await Promise.all(searchTasks.slice(0, 12).map((task) => task())))
    .flat()
    .slice(0, 12);

  return results;
}
