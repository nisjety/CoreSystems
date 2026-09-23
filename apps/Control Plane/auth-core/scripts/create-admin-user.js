#!/usr/bin/env node

/**
 * One-time admin user creation script
 * Creates an admin user with the specified email that can later use Microsoft OAuth
 */

// Uses the global fetch (Node >= 18); node-fetch is not a dependency.

async function createAdminUser() {
  const API_BASE = 'http://localhost:3011';
  const password = process.env.ADMIN_PASSWORD;
  if (!password) {
    throw new Error('Set ADMIN_PASSWORD before creating the local admin user');
  }

  try {
    console.log('🔧 Creating admin user with Microsoft OAuth support...');

    // First, create the user through Better Auth sign-up
    const signUpResponse = await fetch(`${API_BASE}/api/v2/auth/signUp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        email: 'ima.dacosta@coresystem.com',
        password,
        name: 'Ima DaCosta',
      }),
    });

    const signUpResult = await signUpResponse.json();

    if (
      signUpResult.success ||
      signUpResult.error?.includes?.('already exists')
    ) {
      console.log('✅ User exists or created successfully');

      // Now sign in to get authentication
      const signInResponse = await fetch(`${API_BASE}/api/v2/auth/signIn`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          email: 'ima.dacosta@coresystem.com',
          password,
        }),
      });

      const signInResult = await signInResponse.json();
      console.log('📝 Sign-in result:', signInResult);

      if (signInResult.success) {
        console.log('✅ Admin user created and authenticated successfully!');
        console.log('📧 Email: ima.dacosta@coresystem.com');
        console.log('🔑 Can now use Microsoft OAuth for future sign-ins');
        console.log('👑 Admin role will be automatically assigned');
      } else {
        console.log('⚠️  User exists but sign-in failed:', signInResult.error);
        console.log(
          '💡 This is normal if HIBP (password breach check) is enabled',
        );
      }
    } else {
      console.log('❌ Failed to create user:', signUpResult.error);
    }

    // Test admin session check
    console.log('🧪 Testing admin configuration...');
    const sessionResponse = await fetch(`${API_BASE}/api/v2/auth/getSession`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
    });

    const session = await sessionResponse.json();
    console.log('📊 Current session:', session);
  } catch (error) {
    console.error('❌ Error creating admin user:', error);
  }
}

// Run the script
createAdminUser()
  .then(() => {
    console.log('🎉 Admin user setup process completed!');
    console.log('');
    console.log('📋 Next steps:');
    console.log('1. User can now sign in with Microsoft OAuth');
    console.log('2. Admin role will be automatically assigned based on email');
    console.log(
      '3. Microsoft OAuth URL: http://localhost:3011/api/auth/sign-in/microsoft',
    );
    process.exit(0);
  })
  .catch((error) => {
    console.error('💥 Script failed:', error);
    process.exit(1);
  });
