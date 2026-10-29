-- Migration: 003_change_uuid_to_text.down.sql
-- Purpose: Rollback TEXT to UUID (WARNING: This will fail if non-UUID data exists)

-- Step 1: Drop all foreign key constraints
ALTER TABLE user_profiles DROP CONSTRAINT IF EXISTS user_profiles_user_id_fkey;
ALTER TABLE user_sessions DROP CONSTRAINT IF EXISTS user_sessions_user_id_fkey;
ALTER TABLE user_activities DROP CONSTRAINT IF EXISTS user_activities_user_id_fkey;
ALTER TABLE user_roles DROP CONSTRAINT IF EXISTS user_roles_user_id_fkey;
ALTER TABLE user_devices DROP CONSTRAINT IF EXISTS user_devices_user_id_fkey;

-- Step 2: Change users.id from TEXT back to UUID
ALTER TABLE users ALTER COLUMN id TYPE UUID USING id::UUID;
ALTER TABLE users ALTER COLUMN id SET DEFAULT uuid_generate_v4();

-- Step 3: Change all user_id foreign key columns from TEXT back to UUID
ALTER TABLE user_profiles ALTER COLUMN user_id TYPE UUID USING user_id::UUID;
ALTER TABLE user_sessions ALTER COLUMN id TYPE UUID USING id::UUID;
ALTER TABLE user_sessions ALTER COLUMN id SET DEFAULT uuid_generate_v4();
ALTER TABLE user_sessions ALTER COLUMN user_id TYPE UUID USING user_id::UUID;
ALTER TABLE user_activities ALTER COLUMN id TYPE UUID USING id::UUID;
ALTER TABLE user_activities ALTER COLUMN id SET DEFAULT uuid_generate_v4();
ALTER TABLE user_activities ALTER COLUMN user_id TYPE UUID USING user_id::UUID;
ALTER TABLE user_devices ALTER COLUMN id TYPE UUID USING id::UUID;
ALTER TABLE user_devices ALTER COLUMN id SET DEFAULT uuid_generate_v4();
ALTER TABLE user_devices ALTER COLUMN user_id TYPE UUID USING user_id::UUID;

-- Step 4: Change role IDs from TEXT back to UUID
ALTER TABLE roles ALTER COLUMN id TYPE UUID USING id::UUID;
ALTER TABLE roles ALTER COLUMN id SET DEFAULT uuid_generate_v4();

-- Step 5: Change user_roles foreign keys from TEXT back to UUID  
ALTER TABLE user_roles ALTER COLUMN user_id TYPE UUID USING user_id::UUID;
ALTER TABLE user_roles ALTER COLUMN role_id TYPE UUID USING role_id::UUID;

-- Step 6: Re-add foreign key constraints
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
