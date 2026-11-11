#!/usr/bin/env python3
"""The link-preview picture, static/og.png (1200 x 630), from content/site.json.

    python3 site/rendering/social_card.py

The words are the site's headline and its `card` line, set in DejaVu Sans; the
picture is the first frame of the opening scene. Needs ffmpeg with librsvg.
"""
import json
import os
import subprocess
import tempfile
from html import escape

HERE = os.path.dirname(os.path.abspath(__file__))
SITE = os.path.dirname(HERE)
PAGE, FG, BRAND, MUTED = "#060a09", "#f1f6f4", "#5ccbaa", "#9bb0aa"
LOGO = ('<path d="M72 62C90 62 92 88 106 89c9 1 13-9 19-14-2 11-8 24-20 24-17 0-19-19-37-20Z" fill="#2fbf9f"/><circle cx="50" cy="58" r="30" fill="#2fbf9f"/>'
        '<circle cx="60" cy="49" r="9.5" fill="#fff"/><circle cx="62.500" cy="49" r="4.8" fill="#0B2E2A"/>')


def main():
    copy = json.load(open(os.path.join(SITE, "content/site.json")))
    a, b = copy["hero"]["headline"]
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630" font-family="DejaVu Sans">
<g transform="translate(64 62) scale(0.42)">{LOGO}</g>
<text x="130" y="102" font-size="34" font-weight="bold" fill="{FG}">Cadpole</text>
<text x="64" y="252" font-size="68" font-weight="bold" fill="{FG}">{escape(a)}</text>
<text x="64" y="330" font-size="68" font-weight="bold" fill="{BRAND}">{escape(b)}</text>
<text x="64" y="566" font-size="22" font-family="DejaVu Sans Mono" fill="{MUTED}">{escape(copy["card"])}</text>
</svg>'''
    with tempfile.TemporaryDirectory() as tmp:
        words = os.path.join(tmp, "words.svg")
        with open(words, "w") as f:
            f.write(svg)
        # The scene's frame is 16:9 on the page's colour: set it to the right, under the words.
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", f"color=c={PAGE.replace('#', '0x')}:s=1200x630",
                        "-i", os.path.join(SITE, "static/seq/tj/d/000.avif"), "-i", words,
                        "-filter_complex", "[1:v]scale=860:-2[p];[0:v][p]overlay=420:150[bg];[bg][2:v]overlay=0:0,format=rgb24",
                        "-frames:v", "1", "-map_metadata", "-1", os.path.join(SITE, "static/og.png")], check=True)
    print("wrote site/static/og.png")


main()
