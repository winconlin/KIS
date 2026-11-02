#!/usr/bin/env python3
"""Prüft ein gebautes KIS-Image mit eigenem temporären Docker-Volume."""
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


def run(*arguments, **kwargs):
    return subprocess.run(["docker", *arguments], check=True, capture_output=True, text=True, **kwargs).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", default="kis-check")
    args = parser.parse_args()
    suffix = uuid.uuid4().hex
    container = "kis-smoke-" + suffix
    volume = "kis-smoke-data-" + suffix
    environment = dict(os.environ,
        KIS_JWT_SECRET=secrets.token_urlsafe(48),
        KIS_MASTER_KEY=base64.b64encode(secrets.token_bytes(32)).decode())
    password = secrets.token_urlsafe(24)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    run("volume", "create", volume)
    try:
        run("run", "--rm", "-i", "--volume", volume + ":/data/data",
            "--entrypoint", "kis-admin", args.image, "admin", "admin@example.test", input=password + "\n")
        run("run", "--detach", "--name", container, "--publish", "127.0.0.1::8080",
            "--volume", volume + ":/data/data", "--env", "KIS_JWT_SECRET", "--env", "KIS_MASTER_KEY",
            args.image, env=environment)
        address = run("port", container, "8080/tcp")

        def request(path, body=None, token=None):
            headers = {}
            if token:
                headers["Authorization"] = "Bearer " + token
            data = None
            if body is not None:
                data = json.dumps(body).encode()
                headers["Content-Type"] = "application/json"
            req = urllib.request.Request("http://" + address + path, data=data, headers=headers)
            try:
                with opener.open(req, timeout=5) as response:
                    return response.status, response.read()
            except urllib.error.HTTPError as error:
                return error.code, error.read()

        def wait_ready():
            for _ in range(100):
                if run("inspect", "--format", "{{.State.Running}}", container) != "true":
                    raise RuntimeError("KIS-Container vor Erreichbarkeit beendet")
                try:
                    if request("/api/health") == (200, b"OK"):
                        return
                except OSError:
                    pass
                time.sleep(0.1)
            raise RuntimeError("KIS-Container wird nicht erreichbar")

        wait_ready()
        assert run("exec", container, "id", "-u") != "0", "Server läuft unerwartet als root"
        assert run("exec", container, "curl", "-fsS", "http://127.0.0.1:8080/api/health") == "OK"
        assert request("/api/api/health")[0] == 404
        status, body = request("/api/auth/login", {"username": "admin", "password": password})
        assert status == 200, "Anmeldung im Container fehlgeschlagen"
        pair = json.loads(body)
        assert request("/api/patients", token=pair["refresh_token"])[0] == 401
        assert request("/api/patients", token=pair["access_token"])[0] == 501
        assert request("/api/auth/logout", {"refresh_token": pair["refresh_token"]}, pair["access_token"])[0] == 204
        run("restart", container)
        # Nach Neustart kann Docker den zuvor vergebenen Host-Port neu zuweisen.
        address = run("port", container, "8080/tcp")
        wait_ready()
        assert request("/api/patients", token=pair["access_token"])[0] == 401
        assert request("/api/auth/refresh", {"refresh_token": pair["refresh_token"]})[0] == 401
        print("Container-Smoke-Test bestanden: Bootstrap, Non-root, Health, Auth und persistenter Widerruf.")
    finally:
        subprocess.run(["docker", "rm", "--force", container], capture_output=True, check=False)
        run("volume", "rm", volume)


if __name__ == "__main__":
    main()
