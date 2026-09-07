#!/usr/bin/env bash
# ontology-lint — checks shared-vocabulary discipline against src/docs/reference/ONTOLOGY.md
#
# Leg 1 (structure): ONTOLOGY.md exists, has an Owns: header, and every term
#   heading carries a definition line.
# Leg 2 (drift): banned synonyms from the ontology's "Not:" fields must not
#   appear in project docs (src/docs/**/*.md, README.md, AGENTS.md) as
#   standalone references to the concept. Only files OTHER than the ontology
#   itself are checked.
#
# rc semantics: structure violations gate (rc 1). Synonym drift is ADVISORY —
# reported on stderr, does not fail (compositional phrases can be legitimate).
#
# rc 0 = clean · rc 1 = structure violations · rc 2 = usage error.
# Exits 0 on --selftest.

set -u
REPO="${1:-}"
if [ "$REPO" = "--selftest" ]; then
    # selftest: ontology absent → rc 1 expected; structure check works on a fixture
    T=$(mktemp -d)
    if bash "$0" "$T" >/dev/null 2>&1; then echo "selftest FAIL: empty repo passed" >&2; exit 1; fi
    mkdir -p "$T/src/docs/reference"
    printf '# Ontology\n\n> **Owns:** test\n\n**Alpha** — a test term.\n' > "$T/src/docs/reference/ONTOLOGY.md"
    if ! bash "$0" "$T" >/dev/null 2>&1; then echo "selftest FAIL: fixture rejected" >&2; exit 1; fi
    printf 'using the plan file for storage\n' > "$T/src/docs/note.md"
    printf '**Plan** — a session plan.\n\n**Not:** "plan file"\n' >> "$T/src/docs/reference/ONTOLOGY.md"
    OUT=$(bash "$0" "$T" 2>&1)
    echo "$OUT" | grep -q "banned synonym 'plan file'" || { echo "selftest FAIL: banned synonym missed" >&2; exit 1; }
    rm -rf "$T"
    echo "selftest PASS"
    exit 0
fi
if [ -z "$REPO" ] || [ ! -d "$REPO" ]; then
    echo "usage: $0 <repo-path> | $0 --selftest" >&2
    exit 2
fi

ONTOLOGY="$REPO/src/docs/reference/ONTOLOGY.md"
VIOLATIONS=0

fail() { echo "ONTOLOGY-LINT: $1" >&2; VIOLATIONS=$((VIOLATIONS+1)); }

# --- Leg 1: structure ---
if [ ! -f "$ONTOLOGY" ]; then
    fail "src/docs/reference/ONTOLOGY.md missing — the vocabulary SSOT must exist"
    echo "$VIOLATIONS violation(s)" >&2
    exit 1
fi
grep -q '\*\*Owns:\*\*' "$ONTOLOGY" || fail "ONTOLOGY.md has no '**Owns:**' header (ownership law)"
# every **Term** definition line must carry an em-dash definition
grep -E '^\*\*[^*]+\*\* — ' "$ONTOLOGY" >/dev/null 2>&1 || \
    fail "no term definitions found (expected '**Term** — definition' lines)"

# --- Leg 2: banned synonyms in docs ---
# Harvests QUOTED synonyms from '**Not:** "x", "y"' lines. Enforced class =
# MULTI-WORD synonyms only ("plan file", "task list"): these are unambiguously
# wrong names. Single-word synonyms ("task", "chat") have legitimate generic
# English uses and are review discipline, not lint material.
DOCS=$(mktemp)
find "$REPO/src/docs" "$REPO/README.md" "$REPO/AGENTS.md" -name '*.md' -type f 2>/dev/null \
    ! -path "$ONTOLOGY" > "$DOCS"
python3 - "$ONTOLOGY" "$DOCS" <<'PYEOF' > /tmp/ontology-lint.$$ 2>&1
import re, sys, pathlib
onto_path, docs_list = pathlib.Path(sys.argv[1]), sys.argv[2]
text = onto_path.read_text(encoding="utf-8")
# Drop the "How to read this file" legend — its example cell is doc, not rule.
text = re.sub(r"## How to read this file.*?(?=\n## )", "", text, flags=re.S)
banned = []
for line in text.splitlines():
    m = re.search(r"\*\*Not:\*\*\s*(.+)$", line)
    if not m:
        continue
    for syn in re.finditer(r'"([^"]+)"', m.group(1)):
        s = syn.group(1).strip()
        if " " in s:  # multi-word = enforced class
            banned.append(s)
hits = []
for line in pathlib.Path(docs_list).read_text().splitlines():
    p = pathlib.Path(line)
    if not p.is_file():
        continue
    try:
        body = p.read_text(encoding="utf-8")
    except Exception:
        continue
    for syn in banned:
        # whole-word, case-insensitive; skip matches inside code spans/fences crudely
        stripped = re.sub(r"`[^`]*`", "", body)
        if re.search(r"(?i)(?<![`\w])" + re.escape(syn) + r"(?![`\w])", stripped):
            hits.append(f"{p.relative_to(onto_path.parents[3])}: banned synonym '{syn}'")
for h in hits:
    print(f"ONTOLOGY-LINT: {h}", file=sys.stderr)
# Drift hits are ADVISORY: compositional phrases ("plan file" = the file
# holding the plan) can be legitimate. Structure failures gate; drift informs.
print(f"ADVISORY_HITS={len(hits)}")
sys.exit(0)
PYEOF
PYOUT=$(cat /tmp/ontology-lint.$$ 2>/dev/null); rm -f /tmp/ontology-lint.$$
HITS=$(printf '%s' "$PYOUT" | grep -o 'ADVISORY_HITS=[0-9]*' | cut -d= -f2)
printf '%s\n' "$PYOUT" | grep '^ONTOLOGY-LINT:' >&2 || true

rm -f "$DOCS"
if [ "$VIOLATIONS" -gt 0 ]; then
    echo "$VIOLATIONS violation(s)" >&2
    exit 1
fi
if [ -n "${HITS:-}" ]; then
    echo "ONTOLOGY-LINT: $HITS advisory drift hit(s) — review before merge (multi-word synonym usage above)"
fi
echo "ONTOLOGY-LINT: structure clean"
exit 0
