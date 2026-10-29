-- Remove onboarding_complete column from users table
-- Migration: 004_add_onboarding_complete.down.sql

DROP INDEX IF EXISTS idx_users_onboarding_complete;

ALTER TABLE users 
DROP COLUMN IF EXISTS onboarding_complete;
