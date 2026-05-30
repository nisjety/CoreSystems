import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';

import { Pool } from 'pg';

const migrationTableName = 'schema_migrations';

export function createPostgresPool(databaseUrl: string): Pool {
  return new Pool({
    connectionString: databaseUrl,
    max: 10,
    idleTimeoutMillis: 30000,
    connectionTimeoutMillis: 5000
  });
}

export async function runSqlMigrations(pool: Pool, migrationsDirectory: string): Promise<void> {
  await pool.query(`
    CREATE TABLE IF NOT EXISTS ${migrationTableName} (
      version TEXT PRIMARY KEY,
      applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
  `);

  const entries = (await readdir(migrationsDirectory))
    .filter((entry) => entry.endsWith('.sql'))
    .sort((left, right) => left.localeCompare(right));

  for (const entry of entries) {
    const sql = await readFile(join(migrationsDirectory, entry), 'utf8');

    if (sql.trim().length === 0) {
      continue;
    }

    const existingMigration = await pool.query(
      `
        SELECT 1
        FROM ${migrationTableName}
        WHERE version = $1
        LIMIT 1
      `,
      [entry]
    );

    if (existingMigration.rowCount && existingMigration.rowCount > 0) {
      continue;
    }

    const client = await pool.connect();

    try {
      await client.query('BEGIN');
      await client.query(sql);
      await client.query(
        `
          INSERT INTO ${migrationTableName} (version)
          VALUES ($1)
        `,
        [entry]
      );
      await client.query('COMMIT');
    } catch (error) {
      await client.query('ROLLBACK');
      throw error;
    } finally {
      client.release();
    }
  }
}