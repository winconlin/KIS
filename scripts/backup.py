#!/usr/bin/env python3
"""Konsistentes SQLite-Backup mit age; vorhandene Dateien werden nur lesend geprüft."""
import argparse
from datetime import datetime, timezone
import hashlib
import hmac
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import uuid

TABLES = {"users", "patients", "doctors", "appointments", "medications", "audit_log", "sessions", "consents"}


def open_existing(path):
    path = Path(path).resolve(strict=True)
    if not path.is_file():
        raise ValueError("Datenbankpfad ist keine Datei")
    # mode=ro verhindert das Anlegen einer leeren DB bei Tippfehlern.
    return sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=5)


def inspect_database(path):
    connection = open_existing(path)
    try:
        if connection.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
            raise ValueError("SQLite-Integritätsprüfung fehlgeschlagen")
        present = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if not TABLES.issubset(present):
            raise ValueError("Backup enthält kein vollständiges KIS-Schema")
        if connection.execute("PRAGMA foreign_key_check").fetchone() is not None:
            raise ValueError("Backup enthält verletzte Fremdschlüssel")
        return {table: connection.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0] for table in sorted(TABLES)}
    finally:
        connection.close()


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def backup(database, destination, recipients):
    source = open_existing(database)
    destination = Path(destination).resolve()
    recipients = Path(recipients).resolve(strict=True)
    destination.mkdir(parents=True, exist_ok=True, mode=0o700)
    name = "kis-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex + ".db.age"
    archive = destination / name
    try:
        with tempfile.TemporaryDirectory(prefix=".kis-backup-", dir=destination) as directory:
            plaintext = Path(directory) / "snapshot.db"
            target = sqlite3.connect(plaintext)
            try:
                source.backup(target)
            finally:
                target.close()
            counts = inspect_database(plaintext)
            encrypted = Path(directory) / "snapshot.age"
            subprocess.run(["age", "--encrypt", "-R", str(recipients), "--output", str(encrypted), str(plaintext)], check=True)
            # Erst vollständige, verschlüsselte Artefakte veröffentlichen.
            checksum = digest(encrypted)
            os.replace(encrypted, archive)
            archive.with_suffix(archive.suffix + ".sha256").write_text(checksum + "  " + name + "\n", encoding="ascii")
            return {"archive": str(archive), "counts": counts}
    finally:
        source.close()


def verify(archive, identity):
    archive = Path(archive).resolve(strict=True)
    identity = Path(identity).resolve(strict=True)
    checksum_file = archive.with_suffix(archive.suffix + ".sha256")
    checksum, name = checksum_file.read_text(encoding="ascii").strip().split("  ", 1)
    if name != archive.name or not hmac.compare_digest(checksum, digest(archive)):
        raise ValueError("Backup-Prüfsumme stimmt nicht überein")
    with tempfile.TemporaryDirectory(prefix="kis-restore-check-") as directory:
        plaintext = Path(directory) / "restored.db"
        subprocess.run(["age", "--decrypt", "--identity", str(identity), "--output", str(plaintext), str(archive)], check=True)
        return {"archive": str(archive), "counts": inspect_database(plaintext)}


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("backup")
    create.add_argument("--database", required=True, type=Path)
    create.add_argument("--destination", required=True, type=Path)
    create.add_argument("--recipients", required=True, type=Path)
    check = commands.add_parser("verify")
    check.add_argument("--archive", required=True, type=Path)
    check.add_argument("--identity", required=True, type=Path)
    args = parser.parse_args()
    try:
        result = backup(args.database, args.destination, args.recipients) if args.command == "backup" else verify(args.archive, args.identity)
    except (OSError, ValueError, sqlite3.Error, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Backup/Prüfung fehlgeschlagen: {error}\n")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
