-- Migration: 003_change_uuid_to_text.up.sql
-- Purpose: Change all UUID columns to TEXT to support Better Auth's base62 IDs

-- Step 1: Drop all foreign key constraints that reference user IDs
ALTER TABLE user_profiles DROP CONSTRAINT IF EXISTS user_profiles_user_id_fkey;
ALTER TABLE user_sessions DROP CONSTRAINT IF EXISTS user_sessions_user_id_fkey;
ALTER TABLE user_activities DROP CONSTRAINT IF EXISTS user_activities_user_id_fkey;
ALTER TABLE user_roles DROP CONSTRAINT IF EXISTS user_roles_user_id_fkey;
ALTER TABLE user_roles DROP CONSTRAINT IF EXISTS user_roles_role_id_fkey;
ALTER TABLE user_devices DROP CONSTRAINT IF EXISTS user_devices_user_id_fkey;
ALTER TABLE user_settings DROP CONSTRAINT IF EXISTS user_settings_user_id_fkey;
ALTER TABLE provider_accounts DROP CONSTRAINT IF EXISTS provider_accounts_user_id_fkey;

-- Step 2: Change users.id from UUID to TEXT
ALTER TABLE users ALTER COLUMN id TYPE TEXT USING id::TEXT;
ALTER TABLE users ALTER COLUMN id DROP DEFAULT;

-- Step 3: Change all user_id foreign key columns from UUID to TEXT
ALTER TABLE user_profiles ALTER COLUMN user_id TYPE TEXT USING user_id::TEXT;
ALTER TABLE user_sessions ALTER COLUMN id TYPE TEXT USING id::TEXT;
ALTER TABLE user_sessions ALTER COLUMN id DROP DEFAULT;
ALTER TABLE user_sessions ALTER COLUMN user_id TYPE TEXT USING user_id::TEXT;
ALTER TABLE user_activities ALTER COLUMN id TYPE TEXT USING id::TEXT;
ALTER TABLE user_activities ALTER COLUMN id DROP DEFAULT;
ALTER TABLE user_activities ALTER COLUMN user_id TYPE TEXT USING user_id::TEXT;
ALTER TABLE user_devices ALTER COLUMN id TYPE TEXT USING id::TEXT;
ALTER TABLE user_devices ALTER COLUMN id DROP DEFAULT;
ALTER TABLE user_devices ALTER COLUMN user_id TYPE TEXT USING user_id::TEXT;

-- Step 4: Change role IDs from UUID to TEXT
ALTER TABLE roles ALTER COLUMN id TYPE TEXT USING id::TEXT;
ALTER TABLE roles ALTER COLUMN id DROP DEFAULT;

-- Step 5: Change user_roles foreign keys from UUID to TEXT  
ALTER TABLE user_roles ALTER COLUMN user_id TYPE TEXT USING user_id::TEXT;
ALTER TABLE user_roles ALTER COLUMN role_id TYPE TEXT USING role_id::TEXT;

-- Step 6: Change  user_settings and provider_accounts (from migration 002)
ALTER TABLE user_settings ALTER COLUMN id TYPE TEXT USING id::TEXT;
ALTER TABLE user_settings ALTER COLUMN id DROP DEFAULT;
ALTER TABLE user_settings ALTER COLUMN user_id TYPE TEXT USING user_id::TEXT;

ALTER TABLE provider_accounts ALTER COLUMN id TYPE TEXT USING id::TEXT;
ALTER TABLE provider_accounts ALTER COLUMN id DROP DEFAULT;
ALTER TABLE provider_accounts ALTER COLUMN user_id TYPE TEXT USING user_id::TEXT;

-- Step 7: Re-add foreign key constraints
ALTER TABLE user_profiles 
  ADD CONSTRAINT user_profiles_user_id_fkey 
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE user_sessions 
  ADD CONSTRAINT user_sessions_user_id_fkey 
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE user_activities 
  ADD CONSTRAINT user_activities_user_id_fkey 
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE user_roles 
  ADD CONSTRAINT user_roles_user_id_fkey 
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE user_roles 
  ADD CONSTRAINT user_roles_role_id_fkey 
  FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE;

ALTER TABLE user_devices 
  ADD CONSTRAINT user_devices_user_id_fkey 
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE user_settings 
  ADD CONSTRAINT user_settings_user_id_fkey 
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE provider_accounts 
  ADD CONSTRAINT provider_accounts_user_id_fkey 
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

-- The indexes are still valid and don't need to be recreated
-- PostgreSQL automatically updates indexes when column types change
