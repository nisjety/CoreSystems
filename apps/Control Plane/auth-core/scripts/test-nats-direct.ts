#!/usr/bin/env tsx

/**
 * Test NATS Direct Request-Reply Pattern
 * 
 * This script tests direct NATS request-reply without NestJS microservices layer
 * to understand the message format and reply mechanism.
 */

import { connect, StringCodec } from 'nats';

async function testNatsRequestReply() {
  console.log('🔌 Connecting to NATS...');
  
  const nc = await connect({ 
    servers: process.env.NATS_URL || 'nats://localhost:4222',
    maxReconnectAttempts: 3,
    reconnectTimeWait: 1000,
  });

  console.log('✅ Connected to NATS');

  const sc = StringCodec();
  const sharedInternalSecret =
    process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY;
  if (!sharedInternalSecret) {
    throw new Error('INTERNAL_SERVICE_SECRET or INTERNAL_API_KEY must be set');
  }

  // Set up subscriber that mimics what NestJS should do
  console.log('👂 Setting up subscriber for service.authenticate...');
  
  const sub = nc.subscribe('service.authenticate');
  
  (async () => {
    for await (const msg of sub) {
      console.log('\n📨 Received message:');
      console.log('  Subject:', msg.subject);
      console.log('  Reply:', msg.reply);
      console.log('  Data:', sc.decode(msg.data));
      
      // Send reply
      const response = JSON.stringify({
        authenticated: true,
        serviceSecret: sharedInternalSecret,
        serviceId: 'admin-service',
      });
      
      if (msg.reply) {
        console.log('📤 Sending reply to:', msg.reply);
        msg.respond(sc.encode(response));
        console.log('✅ Reply sent');
      } else {
        console.log('❌ No reply subject provided!');
      }
    }
  })();

  // Give subscriber time to register
  await new Promise(resolve => setTimeout(resolve, 500));

  // Send request (simulating Go client)
  console.log('\n📤 Sending request...');
  const request = JSON.stringify({
    serviceId: 'admin-service',
    serviceSecret: sharedInternalSecret,
  });

  try {
    const response = await nc.request(
      'service.authenticate',
      sc.encode(request),
      { timeout: 5000 }
    );
    
    console.log('\n✅ Got response:');
    console.log('  Data:', sc.decode(response.data));
  } catch (error) {
    console.error('\n❌ Request failed:', error);
  }

  // Cleanup
  await sub.drain();
  await nc.drain();
  console.log('\n🔌 Disconnected from NATS');
}

testNatsRequestReply().catch(console.error);
