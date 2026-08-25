#!/usr/bin/env python3
"""Regenerate the Quick Reference table in TESTING.md from the test runner.

The table is one row per module and was maintained by hand, so it drifted:
counts stopped matching the suite and new modules never appeared. This asks
cargo to list the tests it actually runs and rewrites the table between its
header and the next section.

Existing titles are preserved. A title already in the table is reused
verbatim, so curated names ("Tests — A2A Agent Card") survive; only modules
with no row get a generated title. Nothing outside the table is touched.

Usage: python3 scripts/gen-testing-table.py [--check]
       --check exits non-zero if the table is stale, for CI.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TESTS_DIR = ROOT / "src" / "tests"
TESTING_MD = ROOT / "TESTING.md"

TABLE_HEADER = "|----------|------:|----------|"
ROW = re.compile(r"^\| (.+?) \| +(\d+) \| `(src/tests/[^`]+)` \|$")


def counts_from_cargo() -> dict[str, int]:
    """Tests per module, as cargo itself enumerates them.

    Counting `#[test]` attributes in each file looked simpler and was wrong in
    both directions: `service_scope_test` declares 11 that cargo never runs,
    while `telegram_acl_test` runs 12 the file does not visibly declare. Ask
    the runner instead of inferring from source, so the table cannot claim
    tests that do not exist or miss ones that do.
    """
    out = subprocess.run(
        ["cargo", "test", "--locked", "--profile", "ci", "--all-features", "--lib",
         "--", "--list"],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if out.returncode != 0:
        sys.stderr.write(out.stderr)
        raise SystemExit("cargo could not list the tests")

    counts: dict[str, int] = {}
    for line in out.stdout.splitlines():
        # `tests::<module>::<name>: test` — anything else is a benchmark, a
        # doc-test, or one of the few tests living outside `src/tests/`.
        if line.endswith(": test") and line.startswith("tests::"):
            module = line.split("::")[1]
            counts[module] = counts.get(module, 0) + 1
    return counts


def existing_titles() -> dict[str, str]:
    """Map `src/tests/<file>.rs` to the title already in the table."""
    titles: dict[str, str] = {}
    for line in TESTING_MD.read_text().splitlines():
        m = ROW.match(line)
        if m:
            titles[m.group(3)] = m.group(1)
    return titles


def generated_title(module: str) -> str:
    words = [w.capitalize() for w in module.removesuffix("_test").split("_")]
    return "Tests — " + " ".join("A2A" if w.lower() == "a2a" else w for w in words)


def build_rows() -> tuple[list[str], int, int]:
    titles = existing_titles()
    counts = counts_from_cargo()
    rows, total = [], 0
    for module, n in sorted(counts.items()):
        rel = f"src/tests/{module}.rs"
        rows.append(f"| {titles.get(rel, generated_title(module))} | {n} | `{rel}` |")
        total += n
    return rows, total, len(counts)


def main() -> int:
    rows, total, modules = build_rows()
    text = TESTING_MD.read_text()
    lines = text.splitlines()

    start = lines.index(TABLE_HEADER) + 1
    end = start
    while end < len(lines) and lines[end].startswith("|"):
        end += 1

    updated = "\n".join(lines[:start] + rows + lines[end:]) + "\n"

    if "--check" in sys.argv:
        if updated != text:
            print(f"TESTING.md is stale: {modules} modules, {total} tests")
            return 1
        print("TESTING.md is current")
        return 0

    TESTING_MD.write_text(updated)
    print(f"TESTING.md: {modules} modules, {total} tests")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
