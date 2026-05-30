-- Initialize databases for Model Plane v2 services.
-- Runs once on first start of reasoning-v2-postgres.

CREATE DATABASE agent_core_v2_db;
CREATE DATABASE execution_core_db;
CREATE DATABASE capability_core_db;
CREATE DATABASE cost_core_v2_db;

GRANT ALL PRIVILEGES ON DATABASE agent_core_v2_db TO reasoning_user;
GRANT ALL PRIVILEGES ON DATABASE execution_core_db TO reasoning_user;
GRANT ALL PRIVILEGES ON DATABASE capability_core_db TO reasoning_user;
GRANT ALL PRIVILEGES ON DATABASE cost_core_v2_db TO reasoning_user;

\c agent_core_v2_db;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

\c execution_core_db;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

\c capability_core_db;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

\c cost_core_v2_db;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";
