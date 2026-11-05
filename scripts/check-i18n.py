#!/usr/bin/env python3
"""Fail if translation catalogs are incomplete or a view holds a literal.

  check-i18n.py             check the repository
  check-i18n.py --write     regenerate the en-XA pseudo-locale, then check
  check-i18n.py --self-test run the checks against scripts/i18n-fixtures
"""

import json
import re
import sys
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).resolve().parent / "i18n-fixtures"
UI = "crates/kubepulse-ui"
LOCALE_DIRECTORIES = (f"{UI}/locales", "crates/kubepulse-builtin/locales")
PSEUDO = "en-XA"

KEY = re.compile(r'\bt!\(\s*"([^"]+)"')
PLACEHOLDER = re.compile(r"%\{\w+\}")
INTERPOLATION = re.compile(r"\{[^{}]*\}")
CALLS = (
    r"\.(?:label|placeholder|tooltip|title|child|item|description)"
    r"|(?:PopupMenuItem|SettingItem|SettingPage)::new"
)
LITERAL = re.compile(rf'(?:{CALLS})\(\s*(?:format!\(\s*)?"((?:[^"\\]|\\.)*)"')
COLUMN_TITLE = re.compile(r'Column::new\(\s*"[^"]*",\s*"((?:[^"\\]|\\.)*)"')
COLOR = re.compile(r"\b(?:rgba?|hsla?)\(")
IDENTITY_COLOR = re.compile(r"\brgb\((?:identity\.color|color|[a-z_]*color)\)")

ACCENTS = str.maketrans(
    "aceinostuyACEINOSTUY",
    "àçéîñöšţûýÀÇÉÎÑÖŠŢÛÝ",
)


def pseudo(text):
    """Accents letters and adds 30% length, leaving placeholders alone."""
    pieces = re.split(f"({PLACEHOLDER.pattern})", text)
    translated = "".join(
        piece if PLACEHOLDER.fullmatch(piece) else piece.translate(ACCENTS)
        for piece in pieces
    )
    padding = "~" * (len(text) * 3 // 10)
    return f"[{translated}{padding}]"


def read_catalog(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_pseudo(root):
    for directory in LOCALE_DIRECTORIES:
        english = root / directory / "en.json"
        if english.exists():
            catalog = {key: pseudo(text) for key, text in read_catalog(english).items()}
            text = json.dumps(catalog, ensure_ascii=False, indent=2) + "\n"
            (root / directory / f"{PSEUDO}.json").write_text(text, encoding="utf-8")


def catalog_problems(directory):
    english_path = directory / "en.json"
    if not english_path.exists():
        return
    english = read_catalog(english_path)
    for path in sorted(directory.glob("*.json")):
        if path == english_path:
            continue
        catalog = read_catalog(path)
        for key in sorted(english.keys() - catalog.keys()):
            yield f"{path.name}: missing key {key}"
        for key in sorted(catalog.keys() - english.keys()):
            yield f"{path.name}: extra key {key}"
        for key in sorted(english.keys() & catalog.keys()):
            wanted = sorted(PLACEHOLDER.findall(english[key]))
            if sorted(PLACEHOLDER.findall(catalog[key])) != wanted:
                yield f"{path.name}: placeholders differ in {key}"


def source_files(root):
    for path in sorted((root / UI / "src").rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        yield path, text.split("#[cfg(test)]")[0]


def line_of(text, match):
    return text.count("\n", 0, match.start()) + 1


def source_problems(root):
    catalog_path = root / UI / "locales" / "en.json"
    catalog = read_catalog(catalog_path) if catalog_path.exists() else {}
    used = set()
    for path, text in source_files(root):
        relative = path.relative_to(root)
        for match in KEY.finditer(text):
            key = match.group(1)
            used.add(key)
            if key not in catalog:
                yield f"{relative}:{line_of(text, match)}: unknown key {key}"
        for match in [*LITERAL.finditer(text), *COLUMN_TITLE.finditer(text)]:
            shown = INTERPOLATION.sub("", match.group(1))
            if re.search(r"[^\W\d_]", shown):
                yield f"{relative}:{line_of(text, match)}: literal text {match.group(1)!r}"
        for match in COLOR.finditer(text):
            if not IDENTITY_COLOR.match(text, match.start()):
                yield f"{relative}:{line_of(text, match)}: literal color"
    for key in sorted(catalog.keys() - used):
        yield f"en.json: unused key {key}"


def problems(root):
    yield from source_problems(root)
    for directory in LOCALE_DIRECTORIES:
        if (root / directory).is_dir():
            yield from catalog_problems(root / directory)


def self_test():
    failed = False
    for fixture in sorted(path for path in FIXTURES.iterdir() if path.is_dir()):
        expected = (fixture / "expected.txt").read_text(encoding="utf-8").strip()
        found = list(problems(fixture))
        passed = (not found) if not expected else any(expected in p for p in found)
        print(f"{'ok' if passed else 'FAILED'} {fixture.name}: {found}")
        failed |= not passed
    return 1 if failed else 0


def main():
    if "--self-test" in sys.argv:
        return self_test()
    if "--write" in sys.argv:
        write_pseudo(REPOSITORY)
    found = list(problems(REPOSITORY))
    for problem in found:
        print(problem, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
