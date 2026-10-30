#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Verify original crate bytes and reproduce the declared patch without network access."""
from pathlib import Path
import hashlib
import json
import subprocess
import tarfile
import tempfile


root = Path(__file__).resolve().parents[1]
manifest = json.loads((root / "HERMES_PATCH.json").read_text())


def sha256(data):
    return hashlib.sha256(data).hexdigest()


archive = root / manifest["upstream_archive"]
assert sha256(archive.read_bytes()) == manifest["upstream_archive_sha256"], "Original crate archive"
originals = {}
with tarfile.open(archive) as source:
    for member in source.getmembers():
        assert member.isfile(), member.name
        name = Path(member.name).relative_to(f"{manifest['crate']}-{manifest['version']}")
        assert ".." not in name.parts, member.name
        stream = source.extractfile(member)
        assert stream is not None, member.name
        originals[name.as_posix()] = stream.read()
assert {name: sha256(data) for name, data in originals.items()} == manifest["upstream_files"]

expected = set(originals) | set(manifest["added"]) | {"HERMES_PATCH.json"}
actual = {
    p.relative_to(root).as_posix()
    for p in root.rglob("*")
    if p.is_file() and not {"target", ".git"}.intersection(p.relative_to(root).parts)
}
assert actual == expected, f"Missing: {expected - actual}; unrecorded: {actual - expected}"
for name, data in originals.items():
    changed = manifest["modified"].get(name)
    if changed:
        assert changed["upstream_sha256"] == sha256(data), name
        assert sha256((root / name).read_bytes()) == changed["patched_sha256"], name
    else:
        assert (root / name).read_bytes() == data, name
for name, record in manifest["added"].items():
    assert sha256((root / name).read_bytes()) == record["sha256"], name
assert (root / manifest["license_file"]).read_bytes() == originals[manifest["license_file"]]

with tempfile.TemporaryDirectory(prefix="hermes-winit-verify-") as temporary:
    tree = Path(temporary)
    for name in manifest["modified"]:
        target = tree / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(originals[name])
    subprocess.run(
        ["patch", "--batch", "--forward", "--strip=1", "--input", str(root / manifest["patch_file"])],
        cwd=tree,
        check=True,
        capture_output=True,
    )
    for name in manifest["modified"]:
        assert (tree / name).read_bytes() == (root / name).read_bytes(), name

print(
    f"winit {manifest['version']}: original archive, {len(originals)} original files, "
    f"{len(manifest['modified'])} source modification, licenses and reproducible patch verified offline"
)
