import "server-only";
import { getAuthDatabasePool, hasAuthDatabaseConfig } from "@/lib/auth/database";

let onboardingTableReady: Promise<void> | null = null;

async function ensureOnboardingCompletionTable() {
  onboardingTableReady ??= getAuthDatabasePool().query(`
    CREATE TABLE IF NOT EXISTS verevon_onboarding_completions (
      user_id TEXT PRIMARY KEY,
      completed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
  `).then(() => undefined);

  return onboardingTableReady;
}

export async function readLocalOnboardingComplete(userId: string) {
  if (!hasAuthDatabaseConfig()) {
    return false;
  }

  await ensureOnboardingCompletionTable();
  const result = await getAuthDatabasePool().query<{ completed_at: Date }>(
    "SELECT completed_at FROM verevon_onboarding_completions WHERE user_id = $1 LIMIT 1",
    [userId],
  );

  return (result.rowCount ?? 0) > 0;
}

export async function markLocalOnboardingComplete(userId: string) {
  await ensureOnboardingCompletionTable();
  await getAuthDatabasePool().query(
    `
      INSERT INTO verevon_onboarding_completions (user_id, completed_at, updated_at)
      VALUES ($1, NOW(), NOW())
      ON CONFLICT (user_id)
      DO UPDATE SET updated_at = NOW()
    `,
    [userId],
  );
}
