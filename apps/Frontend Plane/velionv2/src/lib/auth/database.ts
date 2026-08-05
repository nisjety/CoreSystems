import { Pool } from "pg";

function readOptionalEnv(name: string): string | undefined {
  const value = process.env[name]?.trim();
  return value ? value : undefined;
}

function isBuildPhase() {
  return process.env.NEXT_PHASE === "phase-production-build";
}

let authDatabasePool: Pool | null = null;

export function hasAuthDatabaseConfig() {
  return Boolean(readOptionalEnv("DATABASE_URL"));
}

export function getAuthDatabasePool() {
  const connectionString = readOptionalEnv("DATABASE_URL");

  if (!connectionString) {
    throw new Error("DATABASE_URL must be configured before Verevon can load signed-in user data.");
  }

  authDatabasePool ??= new Pool({
    connectionString,
    max: Number(process.env.AUTH_DATABASE_POOL_MAX ?? 10),
    idleTimeoutMillis: 30_000,
    connectionTimeoutMillis: 5_000,
  });

  return authDatabasePool;
}

export function createAuthDatabase() {
  if (!hasAuthDatabaseConfig()) {
    if (isBuildPhase()) {
      return undefined;
    }

    throw new Error("DATABASE_URL must be configured before Better Auth can accept real sessions.");
  }

  return getAuthDatabasePool();
}
