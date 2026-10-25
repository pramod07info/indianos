#!/usr/bin/env python3
"""Reject stale CI results by comparing exact full Git commit SHAs."""
import argparse
import re
import sys

_SHA = re.compile(r"[0-9a-fA-F]{40}\Z")


def verify_sha(expected: str, observed: str) -> bool:
    """Only exact, well-formed 40-character SHA matches are accepted."""
    return bool(_SHA.fullmatch(expected) and _SHA.fullmatch(observed) and expected.lower() == observed.lower())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected", required=True, help="Full integration head SHA")
    parser.add_argument("--observed", required=True, help="Full SHA reported by CI")
    args = parser.parse_args()
    if not verify_sha(args.expected, args.observed):
        print("CI SHA mismatch or invalid SHA; refusing stale validation", file=sys.stderr)
        return 1
    print("CI SHA matches expected integration commit")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
