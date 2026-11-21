"""Rétablit la scission 0.14.4 / 0.14.5 dans CHANGELOG.md.

En amont, la balise `0.14.4` existe mais sa section avait été fusionnée dans
`0.14.5`. Ce fork rétablit `0.14.3..0.14.4` (3 correctifs) et borne `0.14.5` à
`0.14.4..0.14.5`. Un resync upstream peut réécrire cette zone et perdre la
scission en silence : ce script la remet dans sa forme canonique.

Usage :
    py -3 scripts/fix-changelog-0.14.4.py           # répare si besoin
    py -3 scripts/fix-changelog-0.14.4.py --check   # échoue si non conforme

Sécurité : si la zone contient une puce dont le commit n'est pas l'un des six
correctifs connus, le script refuse de la modifier (code retour 1) : c'est le
signal qu'un vrai changement amont est arrivé et doit être traité à la main.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

CHANGELOG = Path(__file__).resolve().parent.parent / "CHANGELOG.md"

START = "## [0.14.5]"
END = "## [0.14.3]"

# Commits de 0.14.4 (b78f870, f9ac15c, 8d4a00e) et de 0.14.5 (e10c8ec,
# ee60acf, 2be6f04) : les seuls acceptés dans la zone.
KNOWN_COMMITS = {
    "e10c8ec",
    "ee60acf",
    "2be6f04",
    "b78f870",
    "f9ac15c",
    "8d4a00e",
}

CANONICAL = """## [0.14.5](https://github.com/koharu-rs/koharu/compare/0.14.4..0.14.5) - 2025-12-11

### 🐛 Bug Fixes

- Sakura-1.5b-qwen2.5-v1.0 incorrect eos_token_id - ([e10c8ec](https://github.com/Endymi0n74/Koharu/commit/e10c8eca933c167cac457519698346ddcad45c00))

### 📚 Documentation

- Update changelog - ([ee60acf](https://github.com/Endymi0n74/Koharu/commit/ee60acf993c0b6d2538028fc73c1cec239658761))

### ⚙️ Miscellaneous Tasks

- Remove unwrap - ([2be6f04](https://github.com/Endymi0n74/Koharu/commit/2be6f044bdcaf03fa0d78298f9497750a335b8e7))


## [0.14.4](https://github.com/koharu-rs/koharu/compare/0.14.3..0.14.4) - 2025-12-11

### 🐛 Bug Fixes

- Use templates from gguf file - ([b78f870](https://github.com/Endymi0n74/Koharu/commit/b78f8703272e83019abf83cfe0f3b2e144774a22))
- Format code and llm example - ([f9ac15c](https://github.com/Endymi0n74/Koharu/commit/f9ac15c030af2044fae9077f58b2b934687032c5))
- Remove unused deserialize - ([8d4a00e](https://github.com/Endymi0n74/Koharu/commit/8d4a00e8c1805e99f0681cf408adfb571adaffc4))"""

COMMIT_RE = re.compile(r"commit/([0-9a-f]{7,40})\)")


def main(argv: list[str]) -> int:
    check_only = "--check" in argv[1:]
    raw = CHANGELOG.read_bytes().decode("utf-8")
    newline = "\r\n" if "\r\n" in raw else "\n"
    text = raw.replace("\r\n", "\n")

    try:
        start = text.index(START)
        end = text.index(END, start)
    except ValueError:
        print(f"erreur : {START} ou {END} introuvable dans {CHANGELOG.name}", file=sys.stderr)
        return 1

    region = text[start:end]
    trailing = region[len(region.rstrip("\n")):]
    current = region.rstrip("\n")

    if current == CANONICAL:
        print(f"conforme : scission 0.14.4 / 0.14.5 en place dans {CHANGELOG.name}")
        return 0

    if check_only:
        print(
            "non conforme : la scission 0.14.4 / 0.14.5 a été perdue ; "
            "lancer sans --check pour la rétablir",
            file=sys.stderr,
        )
        return 1

    unknown = sorted({c[:7] for c in COMMIT_RE.findall(current)} - KNOWN_COMMITS)
    if unknown:
        print(
            "refus : puces inconnues dans la zone 0.14.4 / 0.14.5 "
            f"({', '.join(unknown)}) ; à traiter à la main",
            file=sys.stderr,
        )
        return 1

    repaired = text[:start] + CANONICAL + (trailing or "\n\n\n") + text[end:]
    CHANGELOG.write_bytes(repaired.replace("\n", newline).encode("utf-8"))
    print(f"réparé : scission 0.14.4 / 0.14.5 rétablie dans {CHANGELOG.name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
