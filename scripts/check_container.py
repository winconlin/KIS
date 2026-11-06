#!/usr/bin/env python3
"""Smoke-Test eines gebauten KIS-Images mit eigener PostgreSQL-Instanz.

Startet ein temporäres Netzwerk mit postgres:16-alpine (Rollen aus
config/postgres/roles.sql) und dem KIS-Image. Prüft: Migration mit
Eigentümerrolle, Admin-Bootstrap, Betrieb ohne root, Health, Token-Zwecke,
Logout-Widerruf über einen Neustart hinweg und den Schreibschutz des
Audit-Trails für die Laufzeitrolle. Alles wird danach entfernt.
"""
import argparse
import base64
import json
import os
import secrets
import subprocess
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def run(*arguments, **kwargs):
    return subprocess.run(["docker", *arguments], check=True, capture_output=True, text=True, **kwargs).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--image", default="kis:ci")
    args = parser.parse_args()
    suffix = uuid.uuid4().hex[:10]
    net, db, app = f"kis-net-{suffix}", f"kis-db-{suffix}", f"kis-app-{suffix}"
    owner_pw, app_pw, pg_pw = (secrets.token_urlsafe(18) for _ in range(3))
    env = dict(
        os.environ,
        KIS_JWT_SECRET=secrets.token_urlsafe(48),
        KIS_MASTER_KEY=base64.b64encode(secrets.token_bytes(32)).decode(),
        KIS_DATABASE__WRITE_URL=f"postgres://kis_app:{app_pw}@{db}:5432/kis",
        KIS_DATABASE__MIGRATION_URL=f"postgres://kis_owner:{owner_pw}@{db}:5432/kis",
        KIS_ADMIN_PASSWORD=secrets.token_urlsafe(18),
        POSTGRES_PASSWORD=pg_pw, KIS_OWNER_PASSWORD=owner_pw, KIS_APP_PASSWORD=app_pw,
    )
    pass_env = ["KIS_JWT_SECRET", "KIS_MASTER_KEY", "KIS_DATABASE__WRITE_URL", "KIS_DATABASE__MIGRATION_URL"]
    env_args = [a for name in pass_env for a in ("--env", name)]
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    run("network", "create", net)
    try:
        run("run", "--detach", "--name", db, "--network", net,
            "--env", "POSTGRES_PASSWORD", "--env", "KIS_OWNER_PASSWORD", "--env", "KIS_APP_PASSWORD",
            "--volume", f"{ROOT}/config/postgres/roles.sql:/kis/roles.sql:ro",
            "--volume", f"{ROOT}/config/postgres/init-roles.sh:/docker-entrypoint-initdb.d/10-kis-roles.sh:ro",
            "postgres:16-alpine", env=env)
        for _ in range(120):
            ok = subprocess.run(["docker", "exec", db, "pg_isready", "-U", "postgres", "-d", "kis"],
                                capture_output=True).returncode == 0
            if ok:
                break
            time.sleep(0.5)
        else:
            raise RuntimeError("PostgreSQL wird nicht bereit")
        time.sleep(1)

        admin = ["run", "--rm", "--network", net, *env_args, "--entrypoint", "kis-admin", args.image]
        run(*admin, "migrate", env=env)
        run("run", "--rm", "--network", net, *env_args, "--env", "KIS_ADMIN_PASSWORD",
            "--entrypoint", "kis-admin", args.image, "create-user", "admin", "admin@example.test", "Admin", "admin",
            env=env)
        run("run", "--detach", "--name", app, "--network", net, "--publish", "127.0.0.1::8080",
            *env_args, args.image, env=env)

        def address():
            return run("port", app, "8080/tcp").splitlines()[0]

        def request(path, body=None, token=None):
            headers = {}
            if token:
                headers["Authorization"] = "Bearer " + token
            data = None
            if body is not None:
                data = json.dumps(body).encode()
                headers["Content-Type"] = "application/json"
            req = urllib.request.Request("http://" + address() + path, data=data, headers=headers)
            try:
                with opener.open(req, timeout=5) as response:
                    return response.status, response.read()
            except urllib.error.HTTPError as error:
                return error.code, error.read()

        def wait_ready():
            for _ in range(150):
                if run("inspect", "--format", "{{.State.Running}}", app) != "true":
                    raise RuntimeError("KIS-Container beendet:\n" + run("logs", app))
                try:
                    if request("/health") == (200, b"ok"):
                        return
                except OSError:
                    pass
                time.sleep(0.2)
            raise RuntimeError("KIS-Container wird nicht erreichbar")

        wait_ready()
        assert run("exec", app, "id", "-u") != "0", "Server läuft unerwartet als root"
        status, body = request("/api/v1/auth/login", {"email": "admin", "password": env["KIS_ADMIN_PASSWORD"]})
        assert status == 200, f"Anmeldung fehlgeschlagen: {status} {body!r}"
        pair = json.loads(body)
        assert request("/api/v1/patients", token=pair["refresh_token"])[0] == 401
        assert request("/api/v1/patients", token=pair["access_token"])[0] == 200
        assert request("/api/v1/auth/logout", {}, pair["access_token"])[0] == 200
        run("restart", app)
        wait_ready()
        assert request("/api/v1/patients", token=pair["access_token"])[0] == 401
        assert request("/api/v1/auth/refresh", {"refresh_token": pair["refresh_token"]})[0] == 401
        denied = subprocess.run(
            ["docker", "exec", "--env", f"PGPASSWORD={app_pw}", db, "psql", "-U", "kis_app", "-h", "127.0.0.1",
             "-d", "kis", "-c", "DELETE FROM audit_logs"], capture_output=True, text=True)
        assert denied.returncode != 0 and "permission denied" in denied.stderr, denied.stderr
        print("Container-Smoke-Test bestanden: Migration, Bootstrap, Non-root, Auth, "
              "persistenter Widerruf, Audit-Schreibschutz.")
    finally:
        for c in (app, db):
            subprocess.run(["docker", "rm", "--force", c], capture_output=True, check=False)
        subprocess.run(["docker", "network", "rm", net], capture_output=True, check=False)


if __name__ == "__main__":
    main()
