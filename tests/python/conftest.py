from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
for extra in (ROOT / "ml", ROOT / "services", ROOT / "services" / "investigator"):
    text = str(extra)
    if text not in sys.path:
        sys.path.insert(0, text)
