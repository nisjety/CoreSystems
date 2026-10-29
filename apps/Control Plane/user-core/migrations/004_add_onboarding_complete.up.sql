-- Add onboarding_complete column to users table
-- Migration: 004_add_onboarding_complete.up.sql

ALTER TABLE users 
ADD COLUMN IF NOT EXISTS onboarding_complete BOOLEAN NOT NULL DEFAULT false;

-- Create index for faster queries
CREATE INDEX IF NOT EXISTS idx_users_onboarding_complete ON users(onboarding_complete);

-- Update existing users to have onboarding_complete = true if they have a name
-- (Assuming users with names have completed onboarding)
UPDATE users 
SET onboarding_complete = true 
WHERE name IS NOT NULL AND name != '';
