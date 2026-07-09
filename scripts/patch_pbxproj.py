#!/usr/bin/env python3
"""Patch Xcode build phase to skip tauri xcode-script if libapp.a already exists."""
import os
import re
import sys

DEFAULT = 'src-tauri/gen/apple/inverter-dashboard.xcodeproj/project.pbxproj'

raw = sys.argv[1] if len(sys.argv) > 1 else DEFAULT
path = os.path.realpath(raw)

project_root = os.path.realpath(os.path.join(os.path.dirname(__file__), '..'))
if os.path.commonpath([path, project_root]) != project_root:
    sys.exit(f"Error: path '{raw}' resolves outside the project root ({project_root})")

with open(path) as f:
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

with open(path, 'w') as f:
    f.write(c)
print('Build phase patched')
