#!/usr/bin/env python3
"""Compatibility entry point for staging the built-in Frigate package."""

# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name

from plugin_package import main, prepare

__all__ = ["prepare"]

if __name__ == "__main__":
    main(default_plugin="frigate")
