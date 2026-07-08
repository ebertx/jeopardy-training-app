#!/usr/bin/env bash
# Apply a backend SQL migration to the database in .env. Usage: scripts/apply-migration.sh backend/migrations/0005_*.sql
set -euo pipefail
cd "$(dirname "$0")/.."
DB_URL=$(grep -m1 '^DATABASE_URL' .env | sed 's/^DATABASE_URL=//; s/"//g')
docker run --rm -i postgres:16 psql "$DB_URL" -v ON_ERROR_STOP=1 -f - < "$1"
