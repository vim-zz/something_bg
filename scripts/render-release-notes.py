#!/usr/bin/env python3
"""Render one release's approved Markdown bullets as safe, embedded Sparkle HTML."""
import argparse
import html
from pathlib import Path
import re


def release_bullets(text, version):
    headings = list(re.finditer(r"^## (v[^\s]+)\s*$", text, re.MULTILINE))
    matches = [i for i, heading in enumerate(headings) if heading[1] == f"v{version}"]
    if len(matches) != 1:
        raise ValueError(f"Expected exactly one release section for v{version}")
    index = matches[0]
    end = headings[index + 1].start() if index + 1 < len(headings) else len(text)
    section = text[headings[index].end():end]
    bullets = []
    for line in section.splitlines():
        if line.startswith("- "):
            bullets.append(line[2:].strip())
        elif line.startswith("  ") and line.strip() and bullets:
            bullets[-1] += " " + line.strip()
    if not 1 <= len(bullets) <= 5 or any(not bullet for bullet in bullets):
        raise ValueError(f"Release v{version} must contain 1–5 nonempty approved bullets")
    return bullets


def inline_html(text):
    # Release notes use a deliberately small Markdown subset: code, bold, emphasis.
    # Escape all other content, including HTML and CDATA terminators.
    tokens = re.compile(r"(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*)")
    pieces = []
    for part in tokens.split(text):
        if part.startswith("`") and part.endswith("`") and len(part) > 2:
            pieces.append("<code>" + html.escape(part[1:-1]) + "</code>")
        elif part.startswith("**") and part.endswith("**") and len(part) > 4:
            pieces.append("<strong>" + inline_html(part[2:-2]) + "</strong>")
        elif part.startswith("*") and part.endswith("*") and len(part) > 2:
            pieces.append("<em>" + inline_html(part[1:-1]) + "</em>")
        else:
            pieces.append(html.escape(part))
    return "".join(pieces)


def render(text, version):
    return "<ul>\n" + "\n".join(
        f"<li>{inline_html(bullet)}</li>" for bullet in release_bullets(text, version)
    ) + "\n</ul>"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("notes", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    try:
        print(render(args.notes.read_text(encoding="utf-8"), args.version))
    except (OSError, ValueError) as error:
        parser.exit(1, f"Release notes error: {error}\n")


if __name__ == "__main__":
    main()
