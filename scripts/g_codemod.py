#!/usr/bin/env python3
"""G codemod: palette::CONST → theme::role(Role::RoleName) migration.

Per-file atomic. Dry-run first, then apply one file at a time.
Each file gets its own commit with clippy verification.
"""

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Build mapping from theme.rs Role enum comments
theme_src = (REPO / "src/tui/render/theme.rs").read_text()
enum_match = re.search(r"pub enum Role \{(.*?)\}", theme_src, re.DOTALL)
assert enum_match, "Role enum not found in theme.rs"

ROLE_MAP = {}  # palette const name → Role variant name
for line in enum_match.group(1).strip().split("\n"):
    m = re.match(r"\s*(\w+)\s*,\s*//\s*palette::(\w+)", line)
    if m:
        ROLE_MAP[m.group(2)] = m.group(1)

assert len(ROLE_MAP) == 43, f"Expected 43 roles, got {len(ROLE_MAP)}"

# Files to skip (palette.rs itself, theme.rs, presets, mission_control/theme.rs)
SKIP_FILES = {
    "src/tui/render/palette.rs",
    "src/tui/render/theme.rs",
    "src/tui/render/presets.rs",
    "src/tui/render/presets_test.rs",
    "src/tui/render/mission_control/theme.rs",  # already aliased, consumers migrate directly
}


def import_path_for(file_path: str) -> str:
    """Determine the correct import path for theme based on file location."""
    if file_path.startswith("src/tui/render/mission_control/"):
        return "use super::super::theme::{self, Role};"
    elif file_path.startswith("src/tui/render/skills_dialog/"):
        return "use super::super::theme::{self, Role};"
    elif file_path.startswith("src/tui/render/profiles_dialog/"):
        return "use super::super::theme::{self, Role};"
    elif file_path.startswith("src/tui/render/"):
        return "use super::theme::{self, Role};"
    else:
        # src/tui/*.rs, src/usage/*.rs, src/tests/*.rs
        return "use crate::tui::render::theme::{self, Role};"


def migrate_file(file_path: str, dry_run: bool = False) -> tuple[int, bool]:
    """Migrate one file. Returns (replacements_made, import_added)."""
    path = REPO / file_path
    src = path.read_text()
    original = src

    # Count and replace palette::X with theme::role(Role::X)
    replacements = 0

    def replacer(m):
        nonlocal replacements
        const_name = m.group(1)
        if const_name in ROLE_MAP:
            replacements += 1
            return f"theme::role(Role::{ROLE_MAP[const_name]})"
        # Not a themeable const (e.g., palette::dim, palette::muted functions)
        return m.group(0)

    src = re.sub(r"palette::(\w+)", replacer, src)

    if replacements == 0:
        return 0, False

    # Add import if not already present
    import_line = import_path_for(file_path)
    import_added = False

    # Remove old palette imports first
    old_import_patterns = [
        "use super::palette;\n",
        "use super::super::palette;\n",
        "use crate::tui::render::palette;\n",
    ]
    for old in old_import_patterns:
        src = src.replace(old, "")

    # Also handle `pub use` re-exports of palette
    src = re.sub(r"pub use crate::tui::render::palette::\{[^}]*\};\n", "", src)

    if import_line not in src:
        # Insert after the last SINGLE-LINE use statement (not inside multi-line blocks)
        lines = src.split("\n")
        # Find the last SINGLE-LINE use statement at column 0 (module-level, not inside functions)
        last_use_idx = -1
        for i, line in enumerate(lines):
            if line.startswith("use ") and line.rstrip().endswith(";"):
                last_use_idx = i

        if last_use_idx >= 0:
            lines.insert(last_use_idx + 1, import_line)
        else:
            # No module-level single-line use — insert after doc comments and
            # after the first multi-line use block (if any)
            insert_idx = 0
            in_doc = True
            for i, line in enumerate(lines):
                stripped = line.strip()
                if in_doc and (stripped.startswith("//!") or stripped.startswith("//") or stripped == ""):
                    insert_idx = i + 1
                else:
                    in_doc = False
                # Skip past multi-line use blocks at the top
                if not in_doc and stripped == "};":
                    insert_idx = i + 1
                    break
                if not in_doc and not stripped.startswith("use ") and stripped != "" and not stripped.startswith("//"):
                    break
            lines.insert(insert_idx, import_line)
        src = "\n".join(lines)
        import_added = True

    if not dry_run and src != original:
        path.write_text(src)

    return replacements, import_added


def main():
    dry_run = "--dry-run" in sys.argv
    target = None
    for arg in sys.argv[1:]:
        if not arg.startswith("--"):
            target = arg

    # Find all target files
    files = []
    for f in sorted(REPO.glob("src/**/*.rs")):
        rel = str(f.relative_to(REPO))
        if rel in SKIP_FILES:
            continue
        content = f.read_text()
        if "palette::" in content:
            files.append(rel)

    if target:
        files = [f for f in files if f == target]

    if dry_run:
        print(f"DRY RUN — {len(files)} files to migrate\n")

    total = 0
    for f in files:
        count, imported = migrate_file(f, dry_run=dry_run)
        if count > 0:
            prefix = "[DRY] " if dry_run else ""
            import_note = " +import" if imported else ""
            print(f"{prefix}{count:3d} replacements in {f}{import_note}")
            total += count

    print(f"\n{'Would migrate' if dry_run else 'Migrated'}: {total} sites across {len(files)} files")


if __name__ == "__main__":
    main()
