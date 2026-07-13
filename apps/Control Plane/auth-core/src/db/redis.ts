import { createClient, RedisClientType } from 'redis';
import * as dotenv from 'dotenv';

dotenv.config();

// Dragonfly speaks the Redis protocol, so the Node redis client is still used.
const cacheUrl =
  process.env.DRAGONFLY_URL ||
  process.env.CACHE_URL ||
  process.env.REDIS_URL ||
  'redis://localhost:6379';

const redis: RedisClientType = createClient({
  url: cacheUrl,
});

redis.on('error', (err) => {
  console.error('Dragonfly cache client error:', err);
});

redis.on('connect', () => {
  console.log('✅ Connected to Dragonfly cache');
});

redis.on('ready', () => {
  console.log('✅ Dragonfly cache client ready');
});

// Initialize connection
let isConnected = false;

const connectRedis = async () => {
  if (!isConnected && !redis.isOpen) {
    try {
      await redis.connect();
      isConnected = true;
      console.log('🔌 Dragonfly cache connection established');
    } catch (error) {
      console.error('❌ Failed to connect to Dragonfly cache:', error);
      throw error;
    }
  }
  return redis;
};

type SessionMetadata = {
  lastActivity: number;
  rememberMe: boolean;
};

function isSessionMetadata(value: unknown): value is SessionMetadata {
  return (
    typeof value === 'object' &&
    value !== null &&
    'lastActivity' in value &&
    typeof value.lastActivity === 'number' &&
    'rememberMe' in value &&
    typeof value.rememberMe === 'boolean'
  );
}

// Secondary Storage implementation for Better Auth with enhanced state management
export const redisSecondaryStorage = {
  get: async (key: string): Promise<string | null> => {
    try {
      await connectRedis();
      console.log(`🔍 [Cache GET] Attempting to retrieve key: ${key}`);
      const value = await redis.get(key);
      if (value) {
        console.log(`✅ [Cache GET] Found value for key: ${key}`);
        console.log(`   Value length: ${value.length} chars`);
      } else {
        console.log(`❌ [Cache GET] No value found for key: ${key}`);
      }
      return value ? value : null;
    } catch (error) {
      console.error('❌ Cache GET error:', error);
      return null;
    }
  },

  set: async (key: string, value: string, ttl?: number): Promise<void> => {
    try {
      await connectRedis();
      console.log(`💾 [Cache SET] Storing key: ${key}`);
      console.log(
        `   Value length: ${value.length} chars, TTL: ${ttl || 'none'} seconds`,
      );
      if (ttl) {
        // Set with expiration time (TTL in seconds)
        await redis.setEx(key, ttl, value);
      } else {
        // Set without expiration
        await redis.set(key, value);
      }
      console.log(`✅ [Cache SET] Successfully stored key: ${key}`);
    } catch (error) {
      console.error('❌ Cache SET error:', error);
      throw error;
    }
  },

  delete: async (key: string): Promise<void> => {
    try {
      await connectRedis();
      await redis.del(key);
    } catch (error) {
      console.error('Redis DELETE error:', error);
      throw error;
    }
  },

  // Enhanced methods for state management
  cleanupExpiredStates: async (): Promise<void> => {
    try {
      await connectRedis();

      // Cleanup expired OAuth state tokens
      const oauthStateKeys = await redis.keys('oauth:state:*');
      for (const key of oauthStateKeys) {
        const ttl = await redis.ttl(key);
        if (ttl === -1) {
          // Key has no expiration, set one
          await redis.expire(
            key,
            parseInt(process.env.OAUTH_STATE_EXPIRES_IN || '600'),
          );
        }
      }

      // Cleanup expired verification tokens
      const verificationKeys = await redis.keys('verification:*');
      for (const key of verificationKeys) {
        const ttl = await redis.ttl(key);
        if (ttl === -1) {
          // Key has no expiration, set one
          await redis.expire(
            key,
            parseInt(process.env.VERIFICATION_STATE_EXPIRES_IN || '1800'),
          );
        }
      }

      console.log('✅ Redis state cleanup completed');
    } catch (error) {
      console.error('Redis cleanup error:', error);
    }
  },

  // Store session with extended TTL
  setSession: async (
    sessionId: string,
    sessionData: string,
    rememberMe: boolean = true,
  ): Promise<void> => {
    try {
      await connectRedis();
      const sessionKey = `session:${sessionId}`;

      // Extended session duration based on remember me
      const ttl = rememberMe
        ? parseInt(process.env.SESSION_EXPIRES_IN || '604800') // 1 week default
        : parseInt(process.env.SESSION_FRESH_AGE || '300') * 12; // 1 hour for non-remember sessions

      await redis.setEx(sessionKey, ttl, sessionData);

      // Store metadata for session management
      await redis.setEx(
        `session:meta:${sessionId}`,
        ttl,
        JSON.stringify({
          created: Date.now(),
          rememberMe,
          lastActivity: Date.now(),
        }),
      );
    } catch (error) {
      console.error('Redis session storage error:', error);
      throw error;
    }
  },

  // Get session with activity tracking
  getSession: async (sessionId: string): Promise<string | null> => {
    try {
      await connectRedis();
      const sessionKey = `session:${sessionId}`;
      const metaKey = `session:meta:${sessionId}`;

      const [sessionData, metaData] = await Promise.all([
        redis.get(sessionKey),
        redis.get(metaKey),
      ]);

      if (sessionData && metaData) {
        const parsedMetadata: unknown = JSON.parse(metaData);
        if (!isSessionMetadata(parsedMetadata)) {
          return sessionData;
        }
        // Preserve the existing activity-window behavior while constructing a
        // validated immutable metadata value instead of mutating parsed JSON.
        const meta = { ...parsedMetadata, lastActivity: Date.now() };

        // Extend TTL based on activity and remember me setting
        const updateAge = parseInt(process.env.SESSION_UPDATE_AGE || '3600');
        if (Date.now() - meta.lastActivity > updateAge * 1000) {
          const ttl = meta.rememberMe
            ? parseInt(process.env.SESSION_EXPIRES_IN || '604800')
            : parseInt(process.env.SESSION_FRESH_AGE || '300') * 12;

          await Promise.all([
            redis.expire(sessionKey, ttl),
            redis.setEx(metaKey, ttl, JSON.stringify(meta)),
          ]);
        }
      }

      return sessionData;
    } catch (error) {
      console.error('Redis session retrieval error:', error);
      return null;
    }
  },

  // Enhanced state storage with automatic cleanup
  setState: async (
    stateId: string,
    stateData: string,
    type: 'oauth' | 'verification' = 'oauth',
  ): Promise<void> => {
    try {
      await connectRedis();

      const ttl =
        type === 'oauth'
          ? parseInt(process.env.OAUTH_STATE_EXPIRES_IN || '600')
          : parseInt(process.env.VERIFICATION_STATE_EXPIRES_IN || '1800');

      const key = `${type}:state:${stateId}`;
      await redis.setEx(key, ttl, stateData);

      // Mark for cleanup tracking
      await redis.setEx(
        `${type}:state:tracking:${stateId}`,
        ttl + 60,
        'tracked',
      );
    } catch (error) {
      console.error('Redis state storage error:', error);
      throw error;
    }
  },

  // Get and automatically delete state (single use)
  getAndDeleteState: async (
    stateId: string,
    type: 'oauth' | 'verification' = 'oauth',
  ): Promise<string | null> => {
    try {
      await connectRedis();

      const key = `${type}:state:${stateId}`;
      const trackingKey = `${type}:state:tracking:${stateId}`;

      const [stateData] = await Promise.all([
        redis.get(key),
        redis.del(key), // Delete immediately after reading
        redis.del(trackingKey), // Remove tracking
      ]);

      return stateData;
    } catch (error) {
      console.error('Redis state retrieval error:', error);
      return null;
    }
  },
};

// Export Redis client for additional usage if needed
export { redis };

// Graceful shutdown
process.on('SIGINT', () => {
  if (isConnected) {
    redis
      .disconnect()
      .then(() => {
        console.log('🔌 Redis connection closed');
      })
      .catch((error) => {
        console.error('Error closing Redis connection:', error);
      });
  }
});

process.on('SIGTERM', () => {
  if (isConnected) {
    redis
      .disconnect()
      .then(() => {
        console.log('🔌 Redis connection closed');
      })
      .catch((error) => {
        console.error('Error closing Redis connection:', error);
      });
  }
});
