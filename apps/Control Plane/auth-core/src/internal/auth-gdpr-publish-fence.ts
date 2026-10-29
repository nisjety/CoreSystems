import type postgres from 'postgres';

type AuthSqlTransaction = postgres.TransactionSql;

export type AuthGdprIdentity = Readonly<{
  kind: 'email' | 'user';
  value: string | null | undefined;
}>;

export function authGdprUserIdentity(
  value: string | null | undefined,
): AuthGdprIdentity {
  return { kind: 'user', value };
}

export function authGdprEmailIdentity(
  value: string | null | undefined,
): AuthGdprIdentity {
  return { kind: 'email', value };
}

export async function lockAuthGdprIdentities(
  tx: AuthSqlTransaction,
  identities: ReadonlyArray<AuthGdprIdentity>,
): Promise<void> {
  const labels = [
    ...new Set(
      identities.flatMap(({ kind, value }) => {
        if (typeof value !== 'string') return [];
        const normalized =
          kind === 'email' ? value.trim().toLowerCase() : value.trim();
        return normalized ? [`auth-gdpr:${kind}:${normalized}`] : [];
      }),
    ),
  ].sort();

  for (const label of labels) {
    await tx`
      SELECT pg_advisory_xact_lock(hashtextextended(${label}, 0))
    `;
  }
}
