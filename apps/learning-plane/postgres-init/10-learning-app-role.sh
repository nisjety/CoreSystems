#!/bin/sh
set -eu

: "${LEARNING_DB_APP_USER:?LEARNING_DB_APP_USER is required}"
: "${LEARNING_DB_APP_PASSWORD:?LEARNING_DB_APP_PASSWORD is required}"

psql \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=ON_ERROR_STOP=1 \
  --set=learning_database="$POSTGRES_DB" \
  --set=learning_app_user="$LEARNING_DB_APP_USER" \
  --set=learning_app_password="$LEARNING_DB_APP_PASSWORD" <<'SQL'
SELECT format(
  'CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT PASSWORD %L',
  :'learning_app_user', :'learning_app_password'
) \gexec
GRANT CONNECT ON DATABASE :"learning_database" TO :"learning_app_user";
GRANT USAGE ON SCHEMA public TO :"learning_app_user";
SQL
