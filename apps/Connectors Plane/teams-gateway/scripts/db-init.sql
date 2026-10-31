-- Manual init if docker init script not used
DO $$
BEGIN
   IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'aquatiq_admin') THEN
      CREATE ROLE aquatiq_admin LOGIN PASSWORD 'aquatiq_password';
   END IF;
END$$;

DO $$
BEGIN
   IF NOT EXISTS (SELECT FROM pg_database WHERE datname = 'aquatiq_hub') THEN
      CREATE DATABASE aquatiq_hub OWNER aquatiq_admin;
   END IF;
END$$;
