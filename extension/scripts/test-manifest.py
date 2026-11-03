#!/usr/bin/env python3
"""Give a temporary extension copy host access to the e2e server (store builds use optional host permissions)."""
import json
import sys

if len(sys.argv) < 3:
    raise SystemExit("usage: test-manifest.py <manifest.json> <origin-pattern> [...]")
path, *patterns = sys.argv[1:]
with open(path) as handle:
    manifest = json.load(handle)
manifest["host_permissions"] = patterns
manifest.pop("optional_host_permissions", None)
with open(path, "w") as handle:
    json.dump(manifest, handle, indent=2)
    handle.write("\n")
print(f"{path}: host_permissions={patterns}")
