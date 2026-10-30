#!/usr/bin/env python3
"""Screenshot actual demo transcripts using Chromium; requires Python Playwright."""
import argparse
import html
import json
from pathlib import Path
import shutil
from playwright.sync_api import sync_playwright

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--output-dir', type=Path, default=Path('docs/images'))
p.add_argument('--chromium', default=shutil.which('chromium'))
a = p.parse_args()
a.output_dir.mkdir(parents=True, exist_ok=True)
with sync_playwright() as pw:
    browser = pw.chromium.launch(executable_path=a.chromium, headless=True, args=['--no-sandbox'])
    page = browser.new_page(viewport={'width':1200,'height':200}, device_scale_factor=1)
    for scene in json.loads((a.work_dir/'scenes.json').read_text()):
        transcript = (a.work_dir/(scene['name']+'.txt')).read_text()
        page.set_content('''<!doctype html><meta charset="utf-8"><style>
        * {box-sizing:border-box} body {margin:0;background:#eef2f6;padding:28px;color:#e4edf7}
        article {background:#101b2c;border-radius:14px;overflow:hidden;box-shadow:0 8px 20px #0b172533}
        header {background:#1b2b43;padding:20px 26px;border-bottom:1px solid #33465e}
        small {font:14px system-ui;color:#8edbcb;letter-spacing:1px}
        h1 {font:600 24px system-ui;margin:8px 0 0}
        pre {font:17px/1.6 'DejaVu Sans Mono',monospace;white-space:pre-wrap;overflow-wrap:anywhere;margin:0;padding:24px 26px}
        footer {font:14px system-ui;color:#a9b8ca;padding:0 26px 20px}
        </style><article><header><small>EXEVIEWER / ACTUAL CLI OUTPUT EXCERPT</small><h1>'''+html.escape(scene['title'])+'''</h1></header><pre>'''+html.escape(transcript)+'''</pre><footer>Reproducible fixture · Linux x86-64 / glibc · Static analysis, no target execution</footer></article>''')
        page.screenshot(path=str(a.output_dir/(scene['name']+'.png')),full_page=True)
    browser.close()
