import postgres from 'postgres';

const fixtureIDPattern = /^[0-9a-f]{48}$/;

export function validateLifecycleFixtureTarget(
  databaseURL: string,
  expectedDatabase: string,
  fixtureID: string,
): void {
  let parsed: URL;
  try {
    parsed = new URL(databaseURL);
  } catch {
    throw new Error('lifecycle target must be a Postgres URL');
  }
  if (!['postgres:', 'postgresql:'].includes(parsed.protocol)) {
    throw new Error('lifecycle target must be a Postgres URL');
  }
  if (!['127.0.0.1', 'localhost'].includes(parsed.hostname.toLowerCase())) {
    throw new Error('lifecycle target must use a loopback host');
  }
  if (
    decodeURIComponent(parsed.pathname).replace(/^\//, '') !== expectedDatabase
  ) {
    throw new Error(`lifecycle target database must be ${expectedDatabase}`);
  }
  if (!fixtureIDPattern.test(fixtureID)) {
    throw new Error('CONTROL_LIFECYCLE_FIXTURE_ID is invalid');
  }
}

export async function verifyLifecycleFixtureMarker(
  sql: ReturnType<typeof postgres>,
  databaseURL: string,
  expectedDatabase: string,
  fixtureID: string,
): Promise<void> {
  validateLifecycleFixtureTarget(databaseURL, expectedDatabase, fixtureID);
  const rows = await sql<Array<{ fixture_id: string }>>`
    SELECT fixture_id
    FROM control_lifecycle_fixture
    WHERE fixture_id = ${fixtureID}
      AND database_name = ${expectedDatabase}
      AND current_database() = ${expectedDatabase}
  `;
  if (rows.length !== 1 || rows[0].fixture_id !== fixtureID) {
    throw new Error('runner-created lifecycle fixture marker is missing');
  }
}
