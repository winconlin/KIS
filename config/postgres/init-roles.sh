#!/bin/sh
# Initialisierung für den offiziellen postgres-Container
# (/docker-entrypoint-initdb.d). Erwartet KIS_OWNER_PASSWORD und
# KIS_APP_PASSWORD in der Umgebung des Datenbank-Containers.
set -eu
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres \
    -v owner_pw="'${KIS_OWNER_PASSWORD}'" -v app_pw="'${KIS_APP_PASSWORD}'" \
    -f /kis/roles.sql
