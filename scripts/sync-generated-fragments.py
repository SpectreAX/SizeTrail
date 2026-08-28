#!/usr/bin/env python3
import argparse
from pathlib import Path
import sys


FRAGMENTS = {
    "support-matrix": Path("docs/generated/support-matrix.md"),
    "fixture-report": Path("docs/generated/fixture-report.md"),
}


def rendered(readme: str) -> str:
    for name, source in FRAGMENTS.items():
        begin = f"<!-- BEGIN GENERATED: {name} -->"
        end = f"<!-- END GENERATED: {name} -->"
        if readme.count(begin) != 1 or readme.count(end) != 1:
            raise ValueError(f"README marker pair is missing or duplicated: {name}")
        prefix, remainder = readme.split(begin, 1)
        _, suffix = remainder.split(end, 1)
        fragment = source.read_text(encoding="utf-8").rstrip("\n")
        readme = f"{prefix}{begin}\n{fragment}\n{end}{suffix}"
    return readme


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    path = Path("README.md")
    current = path.read_text(encoding="utf-8")
    expected = rendered(current)
    if args.write:
        path.write_text(expected, encoding="utf-8")
        return 0
    if current != expected:
        print("README generated fragments are stale", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
