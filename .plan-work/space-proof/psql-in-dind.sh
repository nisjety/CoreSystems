#!/usr/bin/env bash
# usage: psql-in-dind.sh <container> <db>   (SQL on stdin; runs inside the nested daemon only)
MSYS_NO_PATHCONV=1 docker exec -i space-proof-dind sh -c "docker exec -i $1 sh -c 'psql -U \"\$POSTGRES_USER\" -d $2 -At'"
