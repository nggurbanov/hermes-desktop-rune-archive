#!/usr/bin/env python3
"""Verify the pinned source snapshot and every declared local patch, offline."""
from pathlib import Path
import hashlib
import json

root = Path(__file__).resolve().parents[1]
manifest = json.loads((root / 'HERMES_PATCH.json').read_text())
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
expected = set(manifest['upstream_files']) | set(manifest['added']) | {'HERMES_PATCH.json'}
for name, original in manifest['upstream_files'].items():
    patched = manifest['modified'].get(name)
    if patched:
        assert patched['upstream_sha256'] == original, name
        assert digest(root / name) == patched['patched_sha256'], name
    else:
        assert digest(root / name) == original, name
for name, record in manifest['added'].items():
    assert digest(root / name) == record['sha256'], name
for path in root.rglob('*'):
    if path.is_file() and not {'target', '.git'}.intersection(path.relative_to(root).parts):
        assert path.relative_to(root).as_posix() in expected, f'Unrecorded source: {path.relative_to(root)}'
assert digest(root / manifest['license_file']) == manifest['upstream_files'][manifest['license_file']]
print(f"Rune {manifest['revision']}: {len(manifest['upstream_files'])} original inputs, "
      f"{len(manifest['modified'])} declared source patches, {len(manifest['added'])} additions; verified")
