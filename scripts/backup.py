#!/usr/bin/env python3
"""Verschlüsseltes PostgreSQL-Backup (pg_dump + age) und Prüfung.

backup:  pg_dump --format=plain wird direkt in age gestreamt; unverschlüsselte
         Daten berühren nie die Festplatte. Das Archiv wird erst nach
         erfolgreicher Lesbarkeitsprüfung veröffentlicht (atomar per rename),
         daneben eine SHA-256-Prüfsumme.
verify:  Prüfsumme, Entschlüsselung und Inhaltsverzeichnis (pg_restore --list).
         Optional --restore-into: Wiederherstellung in eine LEERE Prüfdatenbank
         mit Zeilenzählung der Kerntabellen (echter Restore-Test).

Zugangsdaten nicht in die URL schreiben (sonst in der Prozessliste sichtbar),
sondern über PGPASSFILE/PGPASSWORD bereitstellen.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import hmac
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid

TABLES = ("users", "patients", "doctors", "appointments", "medications", "audit_logs", "sessions", "consents")


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def require_tools(*names):
    missing = [n for n in names if shutil.which(n) is None]
    if missing:
        raise ValueError("Werkzeuge fehlen: " + ", ".join(missing))


def _filter_compat_lines(stream):
    """Entfernt SET-Anweisungen, die ältere PostgreSQL-Server nicht kennen."""
    for line in stream:
        if not (line.startswith("SET ") and "transaction_timeout" in line):
            yield line


def _filtered_stream(stream):
    """Liefert ein Pipe-Objekt, aus dem gefilterte Bytes gelesen werden können."""
    import threading
    r_fd, w_fd = os.pipe()
    r_file = os.fdopen(r_fd, "rb")

    def _pump():
        w_file = os.fdopen(w_fd, "wb")
        for raw in stream:
            if isinstance(raw, bytes):
                if b"SET transaction_timeout" not in raw:
                    w_file.write(raw)
            else:
                if not (raw.startswith("SET ") and "transaction_timeout" in raw):
                    w_file.write(raw.encode("utf-8"))
        w_file.close()

    t = threading.Thread(target=_pump, daemon=True)
    t.start()
    return r_file


def archive_tables(archive, identity):
    """Entschlüsselt und prüft, ob alle Kerntabellen im Dump vorhanden sind."""
    decrypt = subprocess.Popen(
        ["age", "--decrypt", "--identity", str(identity), str(archive)], stdout=subprocess.PIPE
    )
    plain = decrypt.stdout.read()
    decrypt.stdout.close()
    if decrypt.wait() != 0:
        raise ValueError("Entschlüsselung fehlgeschlagen (falscher Schlüssel oder beschädigtes Archiv)")
    if b"CREATE TABLE" not in plain:
        raise ValueError("Archiv ist kein gültiger pg_dump-Dump")
    found = set()
    for line in plain.decode("utf-8", errors="replace").splitlines():
        if line.startswith("CREATE TABLE public."):
            rest = line[len("CREATE TABLE public."):]
            found.add(rest.split()[0].strip('"'))
    missing = [t for t in TABLES if t not in found]
    if missing:
        raise ValueError("Backup enthält kein vollständiges KIS-Schema, fehlend: " + ", ".join(missing))
    return sorted(found)


def backup(database_url, destination, recipients, identity_for_check=None):
    require_tools("pg_dump", "age")
    destination = Path(destination).resolve()
    recipients = Path(recipients).resolve(strict=True)
    destination.mkdir(parents=True, exist_ok=True, mode=0o700)
    name = "kis-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex + ".dump.age"
    archive = destination / name
    with tempfile.TemporaryDirectory(prefix=".kis-backup-", dir=destination) as directory:
        encrypted = Path(directory) / "snapshot.age"
        dump = subprocess.Popen(
            ["pg_dump", "--format=plain", "--no-owner", "--no-privileges", "--dbname", database_url],
            stdout=subprocess.PIPE,
        )
        enc = subprocess.run(
            ["age", "--encrypt", "-R", str(recipients), "--output", str(encrypted)], stdin=dump.stdout
        )
        dump.stdout.close()
        if dump.wait() != 0:
            raise ValueError("pg_dump fehlgeschlagen")
        if enc.returncode != 0:
            raise ValueError("Verschlüsselung fehlgeschlagen")
        tables = archive_tables(encrypted, identity_for_check) if identity_for_check else None
        checksum = digest(encrypted)
        os.replace(encrypted, archive)
        archive.with_suffix(archive.suffix + ".sha256").write_text(checksum + "  " + name + "\n", encoding="ascii")
    return {"archive": str(archive), "tables": tables}


def verify(archive, identity, restore_into=None):
    require_tools("age", "pg_restore")
    archive = Path(archive).resolve(strict=True)
    identity = Path(identity).resolve(strict=True)
    checksum_file = archive.with_suffix(archive.suffix + ".sha256")
    checksum, name = checksum_file.read_text(encoding="ascii").strip().split("  ", 1)
    if name != archive.name or not hmac.compare_digest(checksum, digest(archive)):
        raise ValueError("Backup-Prüfsumme stimmt nicht überein")
    tables = archive_tables(archive, identity)
    result = {"archive": str(archive), "tables": tables}
    if restore_into:
        require_tools("psql")
        existing = subprocess.run(
            ["psql", "-X", "-At", "-d", restore_into, "-c",
             "SELECT count(*) FROM pg_tables WHERE schemaname = 'public'"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        if existing != "0":
            # Niemals eine Datenbank mit Inhalt überschreiben.
            raise ValueError("Prüfdatenbank ist nicht leer; Restore-Test abgebrochen")
        decrypt = subprocess.Popen(
            ["age", "--decrypt", "--identity", str(identity), str(archive)], stdout=subprocess.PIPE
        )
        restore = subprocess.run(
            ["psql", "-X", "-v", "ON_ERROR_STOP=1", "-d", restore_into],
            stdin=_filtered_stream(decrypt.stdout), capture_output=True, text=True,
        )
        decrypt.stdout.close()
        if decrypt.wait() != 0 or restore.returncode != 0:
            raise ValueError("Restore fehlgeschlagen: " + restore.stderr.strip()[:500])
        counts = {}
        for t in TABLES:
            counts[t] = int(subprocess.run(
                ["psql", "-X", "-At", "-d", restore_into, "-c", f'SELECT count(*) FROM "{t}"'],
                capture_output=True, text=True, check=True,
            ).stdout.strip())
        result["restored_counts"] = counts
    return result


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("backup")
    create.add_argument("--database-url", required=True)
    create.add_argument("--destination", required=True, type=Path)
    create.add_argument("--recipients", required=True, type=Path)
    create.add_argument("--check-identity", type=Path, help="Archiv vor Veröffentlichung mit diesem Schlüssel prüfen")
    check = commands.add_parser("verify")
    check.add_argument("--archive", required=True, type=Path)
    check.add_argument("--identity", required=True, type=Path)
    check.add_argument("--restore-into", help="URL einer LEEREN Prüfdatenbank für einen echten Restore-Test")
    args = parser.parse_args()
    try:
        if args.command == "backup":
            result = backup(args.database_url, args.destination, args.recipients, args.check_identity)
        else:
            result = verify(args.archive, args.identity, args.restore_into)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Backup/Prüfung fehlgeschlagen: {error}\n")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
