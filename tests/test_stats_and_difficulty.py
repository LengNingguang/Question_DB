import unittest
from pathlib import Path

from question_bank.db import connect, initialize_database
from question_bank.difficulty import update_difficulty_scores
from question_bank.importer import import_bundle
from question_bank.stats import aggregate_score_rows, upsert_stats


class StatsAndDifficultyTests(unittest.TestCase):
    def test_stats_and_difficulty_pipeline(self) -> None:
        project_root = Path(__file__).resolve().parents[1]
        bundle = project_root / "samples" / "demo_bundle"
        csv_path = bundle / "stats" / "raw_scores.csv"
        db_path = project_root / "data" / "test_stats_difficulty.db"
        if db_path.exists():
            db_path.unlink()
        initialize_database(db_path)
        import_bundle(bundle, db_path=db_path, dry_run=False)
        rows = aggregate_score_rows(csv_path)
        self.assertEqual(len(rows), 3)
        upsert_stats(db_path, rows, stats_source="sample", stats_version="v1")
        updated = update_difficulty_scores(db_path, method_version="test-v1")
        self.assertEqual(updated, 3)
        with connect(db_path) as conn:
            stat_count = conn.execute("SELECT COUNT(*) FROM question_stats").fetchone()[0]
            difficulty = conn.execute(
                "SELECT derived_score FROM difficulty_scores WHERE question_id = 'QB-2024-E-03'"
            ).fetchone()[0]
        self.assertEqual(stat_count, 3)
        self.assertGreaterEqual(difficulty, 0.0)


if __name__ == "__main__":
    unittest.main()
