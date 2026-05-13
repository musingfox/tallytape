#!/usr/bin/env python3
"""Check cargo-llvm-cov JSON branch coverage against a minimum percent."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


def load_branch_percent(path: Path) -> float:
    try:
        payload: dict[str, Any] = json.loads(path.read_text())
    except FileNotFoundError as error:
        raise ValueError(f"coverage JSON not found: {path}") from error
    except json.JSONDecodeError as error:
        raise ValueError(f"coverage JSON is invalid: {error}") from error

    try:
        branches = payload["data"][0]["totals"]["branches"]
        percent = branches["percent"]
        count = branches["count"]
    except (KeyError, IndexError, TypeError) as error:
        raise ValueError("coverage JSON is missing data[0].totals.branches.percent") from error

    if count <= 0:
        raise ValueError("coverage JSON has no branch coverage counters")
    if not isinstance(percent, (int, float)):
        raise ValueError("branch coverage percent is not numeric")

    return float(percent)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("json_path", type=Path, help="cargo-llvm-cov JSON summary path")
    parser.add_argument(
        "--min",
        dest="minimum",
        type=float,
        required=True,
        help="minimum branch coverage percentage",
    )
    args = parser.parse_args()

    try:
        percent = load_branch_percent(args.json_path)
    except ValueError as error:
        print(f"branch coverage check failed: {error}", file=sys.stderr)
        return 2

    if percent + 1e-9 < args.minimum:
        print(
            f"branch coverage {percent:.2f}% is below required {args.minimum:.2f}%",
            file=sys.stderr,
        )
        return 1

    print(f"branch coverage {percent:.2f}% meets required {args.minimum:.2f}%")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
