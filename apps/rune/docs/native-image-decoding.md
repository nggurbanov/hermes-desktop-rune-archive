# Native untrusted image decoding

`image_previews::SafeImages` prepares images on one background worker before
Rune receives RGBA pixels. Files and Artifacts use this service; chat and child
spectator media reuse its synchronous preparation function inside their
existing background fetch lane and pass only the resulting safe PNG to Rune.
A transport MIME is advisory: PNG/JPEG/etc signatures choose the raster
reader, otherwise the bounded SVG parser must validate the bytes. SVG labelled
as PNG never falls through to Rune's encoded SVG/resource loader.

There are at most 32 outstanding jobs, including completed results awaiting a
poll. Encoded source reservations are capped at 64 MiB in total, 32 MiB per
image; predicted prepared RGBA reservations are independently capped at 64 MiB.
The size prediction uses the bounded output edge squared, so the normal
24-image gallery fits while large detailed requests still get backpressure.
A full queue reports an actual retryable preview error and leaves original
download bytes intact. Only accepted requests receive a feature-visible ID.

Cancellation has a generation check before and after preparation and again
when results are drained. Individual cancellation lets a new detail selection
retire its old decode without cancelling gallery thumbnails. Files commits only
a matching request ID, frozen editor owner and path. Artifacts commits only a
matching latest request ID and artifact key (profile/session/source); resets and
refresh invalidate old jobs even if identical source paths reappear. Both
feature polls drain completed work when their panes are hidden.

Raster headers are inspected before a full decode. Dimensions are limited to
8192 per side and 64 MiB decoded RGBA; decoder allocation limits remain enabled.
Images downsize only when necessary. SVG preparation uses the existing bounded
resvg helper, denied external/local image href resolution, per-resource and
aggregate embedded decode limits. A shared static font database contains only
the application-bundled Inter and IBM Plex Mono, so diagram text renders without
loading arbitrary system font/resource paths. Ordinary trusted scene fixtures
retain their original fixture path.

Original artifact/file bytes are never replaced by the prepared bitmap. The
native shared preview and explicit save/open paths retain the original source
and its owning scope.

## Verification

The contracts cover worker execution off the caller thread, 32 admitted gallery
requests and bounded undrained results, independent source/pixel budgets,
cancellation, stale Files paths/owners and Artifact selection/reset, disguised
SVG/local resource references, compressed raster expansion, ordinary raster
resizing, vector shapes and actual Inter/monospace text pixels. The production
fixture defers 24 SVG transport replies labelled PNG until feature initialization
and verifies detail decoding still retains the original SVG download bytes.

`check-safe-images.mjs` exercises the actual production Artifacts gallery and
detail view in both appearances. `check-subagent-windows.mjs --rich-media
--mislabeled-svg` exercises the authenticated read and real native spectator
window using the same disguised SVG input. The latter is confirmed to fail on
the immutable third-checkpoint binary, where the previous decoder rejects the
mismatched MIME; its fresh-binary run is the corresponding acceptance gate.
No fixture uses a live agent, provider, microphone or physical audio output.

2026-10-29 acceptance passed on the immutable fourth-checkpoint binary
`/tmp/hermes-rune-fourth-acceptance/hermes-rune`, SHA-256
`3c06534d7a645ee72f43e131d8a0c85de8eb149f77f42eccd82e36dd75056ed6`.
The production gallery/detail gate passed all three native renders in
`/tmp/hermes-safe-images-proof-fourth`: 24 ready thumbnails, both appearances,
real selection controls and retained original-file save action. The actual
spectator window gate passed with `--rich-media --mislabeled-svg`, including
one lazy attachment, one scoped image read, copy, tool disclosure, reuse/focus,
theme propagation and detach. Its three direct window captures are in
`/tmp/hermes-safe-images-child-proof-fourth`. All six images were inspected;
Inter and monospace labels are visible inside the SVG diagrams. The previous
third-checkpoint binary fails the same disguised-SVG input. No Rust change was
needed after these native acceptance runs.
