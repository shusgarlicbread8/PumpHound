"""Regression for cross-book control interference in the learning scan."""
from pathlib import Path
import unittest

SOURCE = Path(__file__).resolve().parents[2] / 'services/core-api/src/paper_loop.rs'

class ChampionTruthWiring(unittest.TestCase):
    def test_explorer_dedupe_cannot_skip_champion_evaluation(self):
        scan = SOURCE.read_text().split('async fn scan_learning_entries(', 1)[1].split('async fn persist_learning_book_decision(', 1)[0]
        self.assertLess(scan.index('let champion_decision ='), scan.index('let explorer_key ='))

    def test_champion_input_quality_comes_from_champion_prediction(self):
        scan = SOURCE.read_text().split('let champion_decision =', 1)[1].split('persist_learning_book_decision(', 1)[0]
        self.assertNotIn('information_score.ood_novelty', scan)
        self.assertIn('champion_familiarity', scan)

if __name__ == '__main__':
    unittest.main()
