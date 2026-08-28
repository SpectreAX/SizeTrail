#!/bin/bash
set -euo pipefail

if [[ -e README.md ]]; then
  python3 scripts/sync-generated-fragments.py
fi

python3 - <<'PY'
from pathlib import Path
import re
import sys

roots = [Path(name) for name in ("README.md", "docs", "site") if Path(name).exists()]
files = []
for root in roots:
    if root.is_file():
        files.append(root)
    else:
        files.extend(path for path in root.rglob("*") if path.is_file())

violations = []
allowed = [
    re.compile(r"https?://[^\s)]+"),
    re.compile(r"\b(?:macOS|Rust|schema|release|version|v)\s*v?\d+(?:\.\d+){0,2}(?:-[A-Za-z0-9.-]+)?\b", re.I),
    re.compile(r"\bApache-\d+\.\d+\b", re.I),
    re.compile(r"§\d+(?:\.\d+)*"),
    re.compile(r"\b(?:as of|dated?)\s+\d{4}-\d{2}-\d{2}\b", re.I),
]

for path in files:
    generated = False
    if path.parts[:2] == ("docs", "generated"):
        continue
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if line.startswith("<!-- BEGIN GENERATED:"):
            generated = True
            continue
        if line.startswith("<!-- END GENERATED:"):
            generated = False
            continue
        if generated:
            continue
        remainder = line
        for pattern in allowed:
            remainder = pattern.sub("", remainder)
        if re.search(r"\d", remainder):
            violations.append(f"{path}:{number}:{line}")

if violations:
    print("\n".join(violations))
    print(
        "public numbers require a narrow version/section/date allowance or an exact generated fragment",
        file=sys.stderr,
    )
    raise SystemExit(1)
PY
