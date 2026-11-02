import importlib.util
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("kis_backup", ROOT / "scripts/backup.py")
backup_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backup_module)


@unittest.skipUnless(shutil.which("age") and shutil.which("age-keygen"), "age und age-keygen erforderlich")
class BackupTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name)
        self.database = self.path / "kis.db"
        self.connection = sqlite3.connect(self.database)
        self.addCleanup(self.connection.close)
        self.connection.execute("PRAGMA journal_mode=WAL")
        for migration in sorted((ROOT / "migrations").glob("*.sql")):
            self.connection.executescript(migration.read_text())
        self.identity = self.path / "identity.txt"
        subprocess.run(["age-keygen", "-o", str(self.identity)], capture_output=True, check=True)
        recipient = subprocess.run(["age-keygen", "-y", str(self.identity)], capture_output=True, text=True, check=True).stdout
        self.recipients = self.path / "recipients.txt"
        self.recipients.write_text(recipient)
        self.destination = self.path / "backups"

    def create_backup(self):
        return Path(backup_module.backup(self.database, self.destination, self.recipients)["archive"])

    def test_wal_roundtrip_and_no_plaintext_left_behind(self):
        self.connection.execute("INSERT INTO patients (id, first_name, last_name, birth_date, created_at, updated_at) VALUES ('p1','Test','Person','1980-01-01','2026-11-02','2026-11-02')")
        self.connection.commit()
        archive = self.create_backup()
        self.assertEqual(backup_module.verify(archive, self.identity)["counts"]["patients"], 1)
        self.assertEqual(sorted(p.suffix for p in self.destination.iterdir()), [".age", ".sha256"])

    def test_missing_source_does_not_create_empty_database(self):
        missing = self.path / "missing.db"
        with self.assertRaises(FileNotFoundError):
            backup_module.backup(missing, self.destination, self.recipients)
        self.assertFalse(missing.exists())

    def test_empty_database_is_not_a_successful_kis_backup(self):
        empty = self.path / "empty.db"
        sqlite3.connect(empty).close()
        with self.assertRaises(ValueError):
            backup_module.backup(empty, self.destination, self.recipients)

    def test_corrupted_archive_fails_checksum(self):
        archive = self.create_backup()
        archive.write_bytes(archive.read_bytes() + b"modified")
        with self.assertRaises(ValueError):
            backup_module.verify(archive, self.identity)

    def test_wrong_decryption_key_is_rejected(self):
        archive = self.create_backup()
        wrong_key = self.path / "wrong-key.txt"
        subprocess.run(["age-keygen", "-o", str(wrong_key)], capture_output=True, check=True)
        with self.assertRaises(subprocess.CalledProcessError):
            backup_module.verify(archive, wrong_key)

    def test_missing_archive_is_rejected(self):
        with self.assertRaises(FileNotFoundError):
            backup_module.verify(self.path / "missing.age", self.identity)


if __name__ == "__main__":
    os.umask(0o077)
    unittest.main()
