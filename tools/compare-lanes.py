#!/usr/bin/env python3
"""Summarise the smoke-test lanes into lane-comparison.md.

Extracted from the workflow so the YAML carries no inline heredoc: a stray `run:` key in a
steps block is what made GitHub reject the whole workflow, and keeping the logic in a real
file removes that class of error.
"""
import pathlib
import re
import sys


def summary(path):
    """The check tally from the report's summary block, as plain text.

    Stripping tags first matters: the counts are wrapped in spans, so a naive non-tag capture
    stopped at the first one and reported "48 checks" with the pass/fail totals missing.
    """
    try:
        text = pathlib.Path(path).read_text()
    except OSError:
        return None
    m = re.search(r'<div class="summary[^"]*">(.*?)</div>', text, re.S)
    if not m:
        m = re.search(r"([0-9]+ checks[^<]*)", text)
        return m.group(1).strip() if m else None
    plain = re.sub(r"<[^>]+>", "", m.group(1))
    plain = (plain.replace("&middot;", "-").replace("&nbsp;", " ")
             .replace("&amp;", "&").strip())
    return re.sub(r"\s*-\s*", " - ", plain)


def main():
    mesa = summary("target/reports/gl-smoke.html")
    angle = summary("target/reports-angle/gl-smoke.html")
    if angle is None and pathlib.Path("target/reports-angle/NOT-RUN.txt").exists():
        angle = "did not run: ANGLE unavailable or did not load"

    out = pathlib.Path("target/reports")
    out.mkdir(parents=True, exist_ok=True)
    lines = [
        "# GL smoke lanes",
        "",
        f"- Mesa llvmpipe: {mesa or 'did not run'}",
        f"- ANGLE on lavapipe: {angle or 'did not run'}",
        "",
        "A check that passes on Mesa but fails on ANGLE is the signature of a translation-layer",
        "bug, which is what a real device exercises. Mesa is a native GLES driver and does not",
        "exercise translation at all.",
    ]
    text = "\n".join(lines) + "\n"
    (out / "lane-comparison.md").write_text(text)
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
