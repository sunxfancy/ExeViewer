#!/usr/bin/env python3
"""Check repository-local inline Markdown links/images; never fetch external URLs."""
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

root = Path(__file__).resolve().parents[1]
files = [root/'Readme.md', *sorted((root/'docs').rglob('*.md')), root/'tests/README.md']
errors = []
count = 0
for file in files:
    text = file.read_text()
    for match in re.finditer(r'!?\[[^\]]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)', text):
        url = urlsplit(match.group(1).strip('<>'))
        if url.scheme or url.netloc or not url.path:
            continue
        count += 1
        path = file.parent/unquote(url.path)
        if not path.exists():
            line = text.count('\n', 0, match.start()) + 1
            errors.append(f'{file.relative_to(root)}:{line}: missing {url.path}')
# Keep the original viewer gallery and the new diagnostic demo visible on the homepage.
readme = (root/'Readme.md').read_text()
for image in [*(f'docs/{n}.png' for n in range(1,6)),
              *(f'docs/images/{name}.png' for name in ['01-diagnosis','02-root-cause','03-repair'])]:
    if f']({image})' not in readme:
        errors.append(f'Readme.md: missing screenshot reference {image}')
if errors:
    raise SystemExit('\n'.join(errors))
print(f'Checked {count} local links across {len(files)} Markdown files; all 8 README screenshots present.')
