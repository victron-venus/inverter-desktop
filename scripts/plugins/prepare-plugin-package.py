#!/usr/bin/env python3
"""Stage a selected built-in desktop plugin without building, signing or installing."""

# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name

from plugin_package import main

if __name__ == "__main__":
    main()
