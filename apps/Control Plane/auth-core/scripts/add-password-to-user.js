/**
 * Print the SQL that gives an existing user a credential-provider password.
 *
 * Takes the password and the user id as arguments rather than hardcoding them.
 * It used to carry a literal password and one specific user id, which meant the
 * secret gate refused the file (`generic-api-key` on the SQL line) and the
 * script only ever worked for whichever environment that id came from. Nothing
 * here is a secret now, so the file can live in the repository.
 *
 * Usage:
 *   node scripts/add-password-to-user.js '<password>' '<user-id>'
 *   ADMIN_PASSWORD=... AUTH_USER_ID=... node scripts/add-password-to-user.js
 *
 * The password is read from argv or the environment and never echoed — only the
 * bcrypt hash is printed, which is what the SQL needs.
 */
const bcrypt = require('bcrypt');
const crypto = require('crypto');

const SALT_ROUNDS = 10;

function usage(message) {
  console.error(`${message}

Usage:
  node scripts/add-password-to-user.js '<password>' '<user-id>'
  ADMIN_PASSWORD=... AUTH_USER_ID=... node scripts/add-password-to-user.js`);
  process.exit(1);
}

async function printCredentialSql() {
  const password = process.argv[2] ?? process.env.ADMIN_PASSWORD;
  const userId = process.argv[3] ?? process.env.AUTH_USER_ID;

  if (!password) usage('A password is required (argv[1] or ADMIN_PASSWORD).');
  if (!userId) usage('A user id is required (argv[2] or AUTH_USER_ID).');

  const hashedPassword = await bcrypt.hash(password, SALT_ROUNDS);
  // The account row needs its own id plus the provider-scoped account id.
  const rowId = crypto.randomUUID();
  const accountId = crypto.randomUUID();

  console.log('Account ID:', accountId);
  console.log('\nSQL to insert credential account:');
  console.log(
    `INSERT INTO "account" (id, account_id, provider_id, user_id, password, created_at, updated_at) ` +
      `VALUES ('${rowId}', '${accountId}', 'credential', '${userId}', '${hashedPassword}', NOW(), NOW());`,
  );
}

printCredentialSql().catch((error) => {
  console.error('Failed to hash the password:', error);
  process.exit(1);
});
