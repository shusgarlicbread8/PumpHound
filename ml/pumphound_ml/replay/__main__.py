"""CLI: python -m pumphound_ml.replay <manifest>"""

from __future__ import annotations

import json
import sys

from pumphound_ml.replay.runner import run_replay


def main(argv: list[str] | None = None) -> int:
    args = argv if argv is not None else sys.argv[1:]
    path = args[0] if args else "fixtures/replay/manifest.json"
    report = run_replay(path)
    print(json.dumps(report.to_dict(), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
