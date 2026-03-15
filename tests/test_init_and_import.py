import unittest
from pathlib import Path

from question_bank.db import connect, initialize_database
from question_bank.importer import import_bundle


class InitAndImportTests(unittest.TestCase):
    def test_initialize_and_import_bundle(self) -> None:
        project_root = Path(__file__).resolve().parents[1]
        bundle = project_root / "samples" / "demo_bundle"
        db_path = project_root / "data" / "test_init_import.db"
        if db_path.exists():
            db_path.unlink()
        initialize_database(db_path)
        result = import_bundle(bundle, db_path=db_path, dry_run=False)
        self.assertEqual(result["status"], "committed")
        self.assertEqual(result["imported_questions"], 3)
        with connect(db_path) as conn:
            question_count = conn.execute("SELECT COUNT(*) FROM questions").fetchone()[0]
            asset_count = conn.execute("SELECT COUNT(*) FROM question_assets").fetchone()[0]
        self.assertEqual(question_count, 3)
        self.assertEqual(asset_count, 2)


if __name__ == "__main__":
    unittest.main()
