#!/usr/bin/env python3
"""Build the Chrome Web Store / Edge Add-ons ZIP without external tools."""
from pathlib import Path
import json
import zipfile

root = Path(__file__).resolve().parent.parent
version = json.loads((root / "manifest.json").read_text())["version"]
out = root.parent / "dist" / f"redline-extension-{version}.zip"
out.parent.mkdir(parents=True, exist_ok=True)
if out.exists():
    out.unlink()
paths = [root / "manifest.json"]
paths += sorted((root / "src").rglob("*.js"))
paths += sorted((root / "src").rglob("*.html"))
paths += sorted((root / "src").rglob("*.css"))
paths += sorted((root / "icons").glob("*.png"))
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in paths:
        archive.write(path, path.relative_to(root).as_posix())
print(f"wrote {out} ({out.stat().st_size // 1024} KiB, {len(paths)} files)")
