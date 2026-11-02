#!/usr/bin/env python3
"""Prüft lokale Markdown-Dateiziele und JSON-/TOML-/Shell-Codeblöcke."""
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def main():
    errors = []
    counts = {"documents": 0, "local_links": 0, "json": 0, "toml": 0, "shell": 0}
    for path in sorted([ROOT / "README.md", *ROOT.joinpath("docs").rglob("*.md")]):
        counts["documents"] += 1
        active = None
        body = []
        start = 0
        for number, line in enumerate(path.read_text().splitlines(), 1):
            marker = re.match(r"^\s*(`{3,}|~{3,})(\S*)\s*$", line)
            if marker:
                fence, language = marker.groups()
                if active is None:
                    active = (fence[0], len(fence), language)
                    body = []
                    start = number
                elif fence[0] == active[0] and len(fence) >= active[1] and not language:
                    code = "\n".join(body)
                    try:
                        if active[2] == "json":
                            counts["json"] += 1
                            json.loads(code)
                        elif active[2] == "toml":
                            counts["toml"] += 1
                            tomllib.loads(code)
                        elif active[2] in {"bash", "sh"}:
                            counts["shell"] += 1
                            result = subprocess.run(["bash", "-n"], input=code, text=True, capture_output=True)
                            if result.returncode:
                                raise ValueError(result.stderr.strip())
                    except ValueError as error:
                        errors.append(f"{path.relative_to(ROOT)}:{start}: {error}")
                    active = None
                else:
                    body.append(line)
                continue
            if active is not None:
                body.append(line)
                continue
            for match in re.finditer(r"(?<!!)\[[^\]]+\]\(([^)]+)\)", line):
                url = match.group(1).strip("<>")
                parsed = urlsplit(url)
                if parsed.scheme or url.startswith("//"):
                    continue
                counts["local_links"] += 1
                target = path.parent / unquote(parsed.path) if parsed.path else path
                if not target.exists():
                    errors.append(f"{path.relative_to(ROOT)}:{number}: Ziel fehlt: {url}")
        if active is not None:
            errors.append(f"{path.relative_to(ROOT)}:{start}: Codeblock nicht geschlossen")
    for error in errors:
        print(error, file=sys.stderr)
    print(json.dumps({"checks": counts, "errors": len(errors)}, ensure_ascii=False))
    return bool(errors)


if __name__ == "__main__":
    sys.exit(main())
