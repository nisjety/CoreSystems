#!/usr/bin/env bash
# Deploy/update the Moment sync service on the Hostinger VPS (31.97.38.31).
#
# Run ON the VPS as a user that can use docker and crontab:
#   bash deploy.sh              # clone/pull, build, start, install weekly cron
#   bash deploy.sh --sync-now   # same, then trigger one full sync immediately
#
# One-time prerequisites on the VPS (secrets are gitignored, so git can't
# bring them - copy them in once from your machine):
#   scp .env.production  <vps>:/opt/moment_service/.env
#   scp app/gcp-credentials.json <vps>:/opt/moment_service/app/gcp-credentials.json

set -euo pipefail

REPO_URL="https://github.com/nisjety/moment.git"
REPO_DIR="/opt/moment_service"
COMPOSE_FILE="deploy/docker-compose.prod.yml"
CONTAINER="aquatiq-moment-service"
LOG_FILE="/var/log/moment-sync.log"
CRON_LINE="0 3 * * 0 docker exec ${CONTAINER} python run_sync.py >> ${LOG_FILE} 2>&1"

echo "==> Fetching latest code"
if [ -d "${REPO_DIR}/.git" ]; then
  git -C "${REPO_DIR}" pull --ff-only
else
  git clone "${REPO_URL}" "${REPO_DIR}"
fi
cd "${REPO_DIR}"

echo "==> Checking required secret files (gitignored, must be copied in manually)"
missing=0
if [ ! -f .env ]; then
  echo "    MISSING: ${REPO_DIR}/.env  (copy .env.production from your machine)"
  missing=1
fi
if [ ! -f app/gcp-credentials.json ]; then
  echo "    MISSING: ${REPO_DIR}/app/gcp-credentials.json  (BigQuery service account key)"
  missing=1
fi
if [ "${missing}" -ne 0 ]; then
  echo "    Aborting - copy the files above and re-run."
  exit 1
fi

echo "==> Building and starting the service"
docker compose -f "${COMPOSE_FILE}" up -d --build

echo "==> Installing weekly cron (Sunday 03:00) idempotently"
touch "${LOG_FILE}" 2>/dev/null || sudo touch "${LOG_FILE}"
( crontab -l 2>/dev/null | grep -vF "run_sync.py" ; echo "${CRON_LINE}" ) | crontab -
echo "    Installed: ${CRON_LINE}"

if [ "${1:-}" = "--sync-now" ]; then
  echo "==> Running one full sync now (takes ~5 minutes)"
  docker exec "${CONTAINER}" python run_sync.py | tee -a "${LOG_FILE}"
fi

echo "==> Done. Current crontab:"
crontab -l | grep run_sync.py
echo "==> Container status:"
docker ps --filter "name=${CONTAINER}" --format "table {{.Names}}\t{{.Status}}"
