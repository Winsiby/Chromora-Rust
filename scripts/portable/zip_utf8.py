#!/usr/bin/env python3
"""Write a zip whose entry names are UTF-8 (ZIP general-purpose bit 11).

Usage: zip_utf8.py STAGE_DIR ZIP_PATH

The archive's top-level folder is the stage directory's basename. Python's
zipfile sets bit 11 for names that are not ASCII, which keeps the Chinese
filenames in the portable package intact on Windows.
"""

from __future__ import annotations

import os
import sys
import zipfile


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: zip_utf8.py STAGE_DIR ZIP_PATH", file=sys.stderr)
        return 2
    stage = os.path.abspath(sys.argv[1])
    dest = os.path.abspath(sys.argv[2])
    if not os.path.isdir(stage):
        print(f"stage directory not found: {stage}", file=sys.stderr)
        return 1
    folder = os.path.basename(stage)
    os.makedirs(os.path.dirname(dest) or ".", exist_ok=True)
    if os.path.exists(dest):
        os.remove(dest)
    count = 0
    with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for dirpath, dirnames, filenames in os.walk(stage):
            dirnames.sort()
            for name in sorted(filenames):
                full = os.path.join(dirpath, name)
                rel = os.path.relpath(full, stage).replace(os.sep, "/")
                archive.write(full, f"{folder}/{rel}")
                count += 1
    if count == 0:
        print(f"stage directory is empty: {stage}", file=sys.stderr)
        return 1
    print(f"wrote {dest} ({count} entries)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
