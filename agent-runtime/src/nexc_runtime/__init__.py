"""nexc-engine agent runtime: agents born from a spec at request time."""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version

try:
    __version__ = version("nexc-runtime")
except PackageNotFoundError:  # running from a source tree without installation
    __version__ = "0.0.0"
