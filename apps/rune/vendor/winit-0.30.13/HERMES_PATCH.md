# Hermes X11 cursor cache patch

This is the exact crates.io `winit 0.30.13` package, with one declared modification
to `src/platform_impl/linux/x11/event_processor.rs`. The original Apache-2.0
`LICENSE`, Cargo manifests and all other upstream files retain their original
bytes. Hermes source changes remain under the upstream Apache-2.0 license;
the added verifier and this note use the repository's MIT license, copied in
`HERMES_LICENSE-MIT.txt`.

`HERMES_PATCH.json` records the locked package checksum, upstream VCS revision,
every original file hash, changed file before/after hashes and each addition.
The original `.crate` archive is retained under `upstream/`; verification needs
Python 3 and the standard `patch` utility and performs no network access:

```sh
python3 apps/rune/vendor/winit-0.30.13/scripts/verify-hermes-source.py
```

X11 focus and pointer-enter events emit `CursorMoved` with their event-local
coordinates. Those paths previously left the window's `cursor_pos` cache at an
older point. A later physical motion back to that older point was suppressed by
the normal motion deduplication, although consumers had received a different
point from focus/entry. A click could consequently hit another control or empty
space, clear the real Hermes input focus and leave Ctrl+A/C with no editor.

The patch synchronizes the existing per-window cache with each generated
focus/entry motion before dispatch. It changes no clipboard, editor ownership,
selection, device settings or platform behavior outside the X11 backend.

The regression exercises ordinary full Hermes windows in a private Xvfb/D-Bus/WM
session against a strict loopback source. It returns to identical window-local
coordinates, performs ordinary pointer presses and native Ctrl+A/C, and reads
the exact copied draft through an external GTK client. Actual runtime pointer
observations are compared with X11's real pointer coordinates. A pointer wiggle
or substituted input/page is not the regression path.

```sh
node apps/rune/scripts/check-peer-windows.mjs --clipboard-only --clipboard-windows 2 --clipboard-rounds 3 --assert-pointer-sync --binary /path/to/hermes-rune --out /path/to/fresh/evidence
node apps/rune/scripts/check-peer-windows.mjs --clipboard-diagnostic --binary /path/to/hermes-rune --out /path/to/fresh/full-evidence
```

On Linux/X11, the unpatched full-application regression failed all six copies
and pointer comparisons. The patched immutable application passed all six copies
and pointer comparisons across three rounds in two native windows. The complete
peer-window flow also passed: independent inputs and streams, scoped stop,
Settings, captured source/profile, session and bot window actions, surviving
primary-window close, and normal application quit. The locked offline application
build passed. These results do not establish macOS or Windows runtime acceptance.

The accepted Linux application SHA-256 is
`5f99ea73ab734ce4796106db0ff086af8c422d962ab3862d0fdedb073adb6eb9`.
Local evidence is recorded in `/tmp/hermes-full-copy-regression-red`,
`/tmp/hermes-full-copy-regression-green` and `/tmp/hermes-winit-full-peer-green`;
these temporary directories are not distributed artifacts. The commands above
recreate fresh evidence for another binary.

The original archive and patch permit reproducing the dependency independently
of a Cargo registry checkout; no global Cargo source cache is modified.
