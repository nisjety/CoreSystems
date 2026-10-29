const { hash } = require('crypto');
const bcrypt = require('bcrypt');

async function createPasswordHash() {
  const password = 'AdminPass123!@#';
  const saltRounds = 10;
  
  try {
    const hashedPassword = await bcrypt.hash(password, saltRounds);
    console.log('Hashed password:', hashedPassword);
    
    // Generate a unique account ID for the credential provider
    const accountId = crypto.randomUUID();
    console.log('Account ID:', accountId);
    
    console.log('\nSQL to insert credential account:');
    console.log(`INSERT INTO "account" (id, account_id, provider_id, user_id, password, created_at, updated_at) VALUES ('${crypto.randomUUID()}', '${accountId}', 'credential', '2PyozzQymscxsoKzauQxTy8vKevTWHTc', '${hashedPassword}', NOW(), NOW());`);
    
  } catch (error) {
    console.error('Error hashing password:', error);
  }
}

createPasswordHash();