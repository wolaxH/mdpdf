#!/usr/bin/env python3
"""產生 mdparse 使用的資料表。

- crates/mdparse/src/entities.rs：HTML5 具名字元參照（來源：WHATWG entities.json）
- crates/mdparse/src/unicode.rs：Unicode 標點／符號與空白字元範圍（來源：Python unicodedata）

用法：scripts/gen-tables.py [entities.json 路徑]（省略時自動下載）
"""

import json
import sys
import unicodedata
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates/mdparse/src"
HEADER = "// 由 scripts/gen-tables.py 產生，請勿手動修改。\n\n"


def rust_str(s: str) -> str:
    return '"' + "".join(f"\\u{{{ord(c):x}}}" if not c.isprintable() or c in '"\\' else c for c in s) + '"'


def gen_entities(path: str | None) -> None:
    if path:
        data = json.loads(Path(path).read_text())
    else:
        with urllib.request.urlopen("https://html.spec.whatwg.org/entities.json") as resp:
            data = json.load(resp)
    # CommonMark 只承認以分號結尾的名稱
    entries = sorted((name[1:-1], value["characters"]) for name, value in data.items() if name.endswith(";"))
    lines = [f"    ({rust_str(name)}, {rust_str(chars)}),\n" for name, chars in entries]
    (OUT / "entities.rs").write_text(
        HEADER
        + "/// HTML5 具名字元參照（不含 `&` 與 `;`），依名稱排序。\n"
        + f"pub static ENTITIES: &[(&str, &str); {len(entries)}] = &[\n"
        + "".join(lines)
        + "];\n"
    )
    print(f"entities: {len(entries)}")


def ranges(pred) -> list[tuple[int, int]]:
    out: list[tuple[int, int]] = []
    for cp in range(0x110000):
        if pred(chr(cp)):
            if out and out[-1][1] == cp - 1:
                out[-1] = (out[-1][0], cp)
            else:
                out.append((cp, cp))
    return out


def rust_ranges(name: str, doc: str, rs: list[tuple[int, int]]) -> str:
    body = "".join(f"    ('\\u{{{a:x}}}', '\\u{{{b:x}}}'),\n" for a, b in rs)
    return f"/// {doc}\npub static {name}: &[(char, char)] = &[\n{body}];\n"


def gen_unicode() -> None:
    punct = ranges(lambda c: unicodedata.category(c)[0] in "PS")
    space = ranges(lambda c: unicodedata.category(c) == "Zs" or c in "\t\n\x0c\r")
    (OUT / "unicode.rs").write_text(
        HEADER
        + f"// 產生時的 Unicode 版本：{unicodedata.unidata_version}\n\n"
        + rust_ranges("PUNCTUATION", "Unicode 標點（P*）與符號（S*）類別。", punct)
        + "\n"
        + rust_ranges("WHITESPACE", "Unicode 空白：Zs 類別加上 tab、換行、換頁、歸位。", space)
    )
    print(f"punctuation ranges: {len(punct)}, whitespace ranges: {len(space)}")


if __name__ == "__main__":
    gen_entities(sys.argv[1] if len(sys.argv) > 1 else None)
    gen_unicode()
