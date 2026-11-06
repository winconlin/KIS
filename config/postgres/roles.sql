-- ============================================================
-- KIS — PostgreSQL-Rollen (einmalig als Superuser ausführen)
--
--   psql -v owner_pw="'…'" -v app_pw="'…'" -f config/postgres/roles.sql
--
-- kis_owner : Eigentümer des Schemas, nur für Migrationen
--             (database.migration_url / KIS_DATABASE__MIGRATION_URL)
-- kis_app   : Laufzeitrolle des Servers (database.write_url), nur DML,
--             auf audit_logs nur SELECT/INSERT
-- Passwörter nicht in Dateien ablegen; über -v übergeben.
-- ============================================================

CREATE ROLE kis_owner LOGIN PASSWORD :owner_pw;
CREATE ROLE kis_app   LOGIN PASSWORD :app_pw NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE DATABASE kis OWNER kis_owner;

\connect kis
-- Erweiterungen benötigen Superuser-Rechte; vorab anlegen.
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS btree_gist;

ALTER SCHEMA public OWNER TO kis_owner;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO kis_app;

ALTER DEFAULT PRIVILEGES FOR ROLE kis_owner IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO kis_app;
ALTER DEFAULT PRIVILEGES FOR ROLE kis_owner IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO kis_app;
-- Migration 004 entzieht kis_app anschließend UPDATE/DELETE/TRUNCATE
-- auf audit_logs.
