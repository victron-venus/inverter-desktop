#!/usr/bin/env python3
"""Patch Xcode build phase to skip tauri xcode-script if libapp.a already exists."""
import re
import sys

try:
    from pathlib import Path
except ImportError:
    Path = None

PROJECT = Path(__file__).resolve().parent.parent
ALLOWED = PROJECT / 'src-tauri/gen/apple/inverter-dashboard.xcodeproj/project.pbxproj'

if len(sys.argv) > 1:
    candidate = PROJECT / sys.argv[1]
else:
    candidate = ALLOWED

path = candidate.resolve()
try:
    path.relative_to(PROJECT)
except ValueError:
    sys.exit(f"Error: path resolves outside the project root ({PROJECT})")

with path.open() as f:
    c = f.read()

HEAD = 'shellScript = "'
BODY = 'pnpm tauri ios xcode-script'
INDENT = '\t\t\t\t'

wrap = HEAD + 'if [ ! -f \\"${SRCROOT}/Externals/arm64/${CONFIGURATION}/libapp.a\\" ]; then\n' + INDENT + BODY
c = c.replace(HEAD + BODY, wrap, 1)

c = c.replace(
    '${ARCHS:?}";',
    '${ARCHS:?}\n\t\t\tfi";',
    1
)

with path.open('w') as f:
    f.write(c)
print('Build phase patched')
