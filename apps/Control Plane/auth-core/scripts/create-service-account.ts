#!/usr/bin/env tsx

/**
 * Create Service Account Script
 * 
 * Creates a service account user in Better Auth with:
 * - Email: service@internal.aquatiq.com
 * - Role: admin
 * - No password (cannot be used for regular login)
 * - Long-lived session token for internal services
 */

import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import { eq } from 'drizzle-orm';
import * as schema from '../src/db/schema';
import { randomBytes } from 'crypto';

async function createServiceAccount() {
  const databaseUrl = process.env.DATABASE_URL || 'postgresql://postgres:postgres@localhost:5432/aquatiq_auth';
  
  console.log('🔌 Connecting to database...');
  const client = postgres(databaseUrl);
  const db = drizzle(client, { schema });

  try {
    const serviceEmail = 'service@internal.aquatiq.com';
    const serviceName = 'Internal Service Account';
    const serviceUserId = 'service-account-' + randomBytes(16).toString('hex');
    const sessionToken = randomBytes(32).toString('hex');
    const sessionId = 'session-' + randomBytes(16).toString('hex');

    console.log('👤 Creating service account user...');
    
    // Check if service account already exists
    const existingUser = await db.query.user.findFirst({
      where: eq(schema.user.email, serviceEmail),
    });

    let userId: string;
    if (existingUser) {
      console.log('⚠️  Service account already exists, reusing:', existingUser.id);
      userId = existingUser.id;
      
      // Delete existing sessions
      await db.delete(schema.session).where(eq(schema.session.userId, userId));
      console.log('🗑️  Deleted existing sessions');
    } else {
      // Create service account user
      const [user] = await db.insert(schema.user).values({
        id: serviceUserId,
        email: serviceEmail,
        name: serviceName,
        emailVerified: true,
        role: 'admin',
        banned: false,
        banReason: null,
        banExpires: null,
        createdAt: new Date(),
        updatedAt: new Date(),
      }).returning();
      
      userId = user.id;
      console.log('✅ Service account created:', userId);
    }

    // Create long-lived session (1 year expiry)
    const expiresAt = new Date();
    expiresAt.setFullYear(expiresAt.getFullYear() + 1);

    await db.insert(schema.session).values({
      id: sessionId,
      userId: userId,
      token: sessionToken,
      expiresAt: expiresAt,
      ipAddress: '127.0.0.1',
      userAgent: 'Internal-Service/1.0',
      createdAt: new Date(),
      updatedAt: new Date(),
    });

    console.log('✅ Service session created');
    console.log('\n📋 Service Account Details:');
    console.log('  User ID:', userId);
    console.log('  Email:', serviceEmail);
    console.log('  Session Token:', sessionToken);
    console.log('  Expires:', expiresAt.toISOString());
    console.log('\n🔧 Add to admin-service-go configuration:');
    console.log(`  BETTER_AUTH_SERVICE_SESSION="${sessionToken}"`);
    console.log('\n🔧 Add to docker-compose.yml admin-service-go environment:');
    console.log(`  - BETTER_AUTH_SERVICE_SESSION=${sessionToken}`);
    
  } catch (error) {
    console.error('❌ Failed to create service account:', error);
    process.exit(1);
  } finally {
    await client.end();
  }
}

createServiceAccount();
