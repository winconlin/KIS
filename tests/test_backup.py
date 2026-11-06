"""Tests für scripts/backup.py gegen echte PostgreSQL-Datenbanken.

Voraussetzungen: age, age-keygen, pg_dump, pg_restore, psql und
KIS_TEST_DATABASE_URL (Admin-Verbindung, darf Datenbanken anlegen).
Ohne Voraussetzungen werden die Tests übersprungen (gilt nicht als bestanden);
mit KIS_REQUIRE_PG_TESTS=1 schlagen sie stattdessen fehl.
"""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("kis_backup", ROOT / "scripts/backup.py")
backup_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backup_module)

ADMIN_URL = os.environ.get("KIS_TEST_DATABASE_URL")
TOOLS = all(shutil.which(t) for t in ("age", "age-keygen", "pg_dump", "pg_restore", "psql"))
if os.environ.get("KIS_REQUIRE_PG_TESTS") == "1" and not (ADMIN_URL and TOOLS):
    raise RuntimeError("Backup-Tests sind verpflichtend, Voraussetzungen fehlen")


def with_db(url, name):
    base, _, query = url.partition("?")
    return base.rsplit("/", 1)[0] + "/" + name + (("?" + query) if query else "")


def psql(url, sql):
    return subprocess.run(["psql", "-X", "-At", "-v", "ON_ERROR_STOP=1", "-d", url, "-c", sql],
                          capture_output=True, text=True, check=True).stdout.strip()


@unittest.skipUnless(ADMIN_URL and TOOLS, "PostgreSQL (KIS_TEST_DATABASE_URL) und age/pg-Werkzeuge erforderlich")
class BackupTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name)
        self.names = []
        self.source = self.new_db()
        for migration in sorted((ROOT / "migrations").glob("*.sql")):
            subprocess.run(["psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-d", self.source, "-f", str(migration)],
                           check=True, capture_output=True)
        psql(self.source, "INSERT INTO patients (name_family, name_given, birth_date) VALUES ('Synth', 'Test', '1980-01-01')")
        self.identity = self.path / "identity.txt"
        subprocess.run(["age-keygen", "-o", str(self.identity)], capture_output=True, check=True)
        recipient = subprocess.run(["age-keygen", "-y", str(self.identity)], capture_output=True, text=True, check=True).stdout
        self.recipients = self.path / "recipients.txt"
        self.recipients.write_text(recipient)
        self.destination = self.path / "backups"

    def new_db(self):
        name = "kis_bk_" + uuid.uuid4().hex[:12]
        psql(ADMIN_URL, f"CREATE DATABASE {name}")
        self.names.append(name)
        self.addCleanup(lambda: psql(ADMIN_URL, f"DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        return with_db(ADMIN_URL, name)

    def create_backup(self):
        return Path(backup_module.backup(self.source, self.destination, self.recipients, self.identity)["archive"])

    def test_roundtrip_restore_and_no_plaintext_left_behind(self):
        archive = self.create_backup()
        target = self.new_db()
        result = backup_module.verify(archive, self.identity, restore_into=target)
        self.assertEqual(result["restored_counts"]["patients"], 1)
        self.assertEqual(sorted(p.name for p in self.destination.iterdir()), sorted([archive.name, archive.name + ".sha256"]))
        self.assertNotIn(b"Synth", archive.read_bytes())

    def test_restore_refuses_non_empty_target(self):
        archive = self.create_backup()
        with self.assertRaises(ValueError):
            backup_module.verify(archive, self.identity, restore_into=self.source)
        self.assertEqual(psql(self.source, "SELECT count(*) FROM patients"), "1")

    def test_empty_database_is_not_a_kis_backup(self):
        self.source = self.new_db()
        with self.assertRaises(ValueError):
            self.create_backup()
        self.assertFalse(any(self.destination.glob("*.age")))

    def test_corrupted_archive_fails_checksum(self):
        archive = self.create_backup()
        archive.write_bytes(archive.read_bytes() + b"x")
        with self.assertRaises(ValueError):
            backup_module.verify(archive, self.identity)

    def test_wrong_decryption_key_is_rejected(self):
        archive = self.create_backup()
        other = self.path / "other.txt"
        subprocess.run(["age-keygen", "-o", str(other)], capture_output=True, check=True)
        with self.assertRaises(ValueError):
            backup_module.verify(archive, other)

    def test_missing_archive_is_rejected(self):
        with self.assertRaises(OSError):
            backup_module.verify(self.path / "missing.age", self.identity)


if __name__ == "__main__":
    unittest.main()
