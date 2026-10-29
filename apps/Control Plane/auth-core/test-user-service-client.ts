/**
 * Test script for UserServiceClient
 * This script tests the communication between auth-service and user-service
 */

import { UserServiceClient } from './src/internal/user-service.client';
import { ConfigService } from '@nestjs/config';

class MockConfigService extends ConfigService {
  get(key: string): string {
    if (key === 'USER_SERVICE_URL') {
      return 'http://localhost:3012';
    }
    return '';
  }
}

async function testUserServiceClient() {
  console.log('🧪 Testing UserServiceClient...\n');

  const configService = new MockConfigService();
  const userServiceClient = new UserServiceClient(configService);

  // Test 1: Health Check
  console.log('1️⃣ Testing health check...');
  try {
    const health = await userServiceClient.healthCheck();
    console.log('✅ Health check successful:', health);
  } catch (error) {
    console.log('❌ Health check failed:', (error as Error).message);
  }

  // Test 2: Sync User
  console.log('\n2️⃣ Testing user sync...');
  try {
    const success = await userServiceClient.syncUser({
      authUserId: 'test-user-123',
      email: 'test@example.com',
      name: 'Test User',
      emailVerified: true,
    });
    console.log('✅ User sync result:', success);
  } catch (error) {
    console.log('❌ User sync failed:', (error as Error).message);
  }

  // Test 3: Validate User Exists
  console.log('\n3️⃣ Testing user exists validation...');
  try {
    const exists =
      await userServiceClient.validateUserExists('test@example.com');
    console.log('✅ User exists check:', exists);
  } catch (error) {
    console.log('❌ User exists check failed:', (error as Error).message);
  }

  console.log('\n🏁 UserServiceClient tests completed!');
}

// Run the tests
testUserServiceClient().catch(console.error);
