#!/usr/bin/env python3
"""Extract the GFM extension examples from the cmark-gfm test files into tests/spec/gfm-extensions.json.

Usage: scripts/extract-gfm-spec.py <cmark-gfm/test/spec.txt> <cmark-gfm/test/extensions.txt>
"""

import json
import re
import sys
from pathlib import Path

FENCE = "`" * 32
SPEC_TAGS = {"table", "strikethrough", "disabled"}
EXT_SECTIONS = {"Tables", "Strikethroughs", "Task lists"}


def examples(path: str):
    section = ""
    lines = Path(path).read_text().split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        if m := re.match(r"^#{1,6} (.*)", line):
            section = m.group(1).strip()
        if line.startswith(FENCE + " example"):
            tag = line[len(FENCE) + len(" example"):].strip()
            j = lines.index(".", i + 1)
            k = lines.index(FENCE, j + 1)
            md = "\n".join(lines[i + 1 : j]) + "\n"
            html = "\n".join(lines[j + 1 : k]) + "\n"
            yield section, tag, md.replace("→", "\t"), html.replace("→", "\t"), i + 1
            i = k
        i += 1


def main(spec: str, ext: str) -> None:
    out = []
    for section, tag, md, html, line in examples(spec):
        if tag in SPEC_TAGS:
            out.append({"markdown": md, "html": html, "section": f"spec: {section}", "line": line})
    for section, _tag, md, html, line in examples(ext):
        if section in EXT_SECTIONS:
            out.append({"markdown": md, "html": html, "section": f"extensions: {section}", "line": line})
    dest = Path(__file__).resolve().parent.parent / "tests/spec/gfm-extensions.json"
    dest.write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n")
    print(f"{len(out)} examples -> {dest}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
