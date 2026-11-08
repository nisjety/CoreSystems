-- PostgreSQL initialization script for Ingestion Stack
-- This script runs automatically when PostgreSQL starts for the first time

-- Create imports database user and database
CREATE ROLE imports WITH LOGIN PASSWORD 'imports';
CREATE ROLE quarry WITH LOGIN PASSWORD 'quarry';

CREATE DATABASE imports OWNER imports;
CREATE DATABASE quarry OWNER quarry;

-- Grant privileges
GRANT ALL PRIVILEGES ON DATABASE imports TO imports;
GRANT ALL PRIVILEGES ON DATABASE quarry TO quarry;
