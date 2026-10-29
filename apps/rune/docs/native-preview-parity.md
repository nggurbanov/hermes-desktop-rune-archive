# Native file previews

The shared `preview::Service` owns a frozen connection/profile/session target,
not the foreground chat. Files opens it only after the editor's existing
dirty/busy guard; Artifacts explicitly opens the same service. The original
file and the rendered image have separate lifetimes and representations.

## Images and transport

- Native Rune canvas: fit, actual size, 25–800% zoom, pointer-centered wheel and
  pinch, smart magnify, primary/middle-button pan, keyboard `+`, `-`, `0`, `1`.
  Toolbar actions use the OpenCode kit and both actual appearances. Owner and
  source remain visible when the foreground conversation changes.
- Original copies are private process-owned files, with 0700 directories and
  0600 files on Unix. Save writes the original bytes through an atomic sibling
  temporary file, and rejects stale dialog IDs. Closing or retiring the owning
  connection/profile cancels transfer and drops the copy.
- Two background jobs maximum, one pending latest target, bounded reply queue,
  and full owner/request-ID fencing. A cancelled body does not wait for the
  remote peer to deliver its next chunk. Native auth preparation retains the
  existing bounded gateway discovery/ticket deadlines.
- `Gateway::stream_file` supports GET/HEAD, Range, If-Range, If-None-Match,
  If-Modified-Since; preserves HTTP status, Content-Range, Content-Length,
  Accept-Ranges, ETag and Last-Modified. It never follows redirects. Only the
  selected gateway may receive credentials; agent paths become a query value,
  never a replacement route. Every native request gets a fresh scoped ticket.
- Image originals use `/api/fs/download`; a 404/405 permits the baseline capped
  `/api/fs/read-data-url` fallback. The raw media route is `/api/files/stream`.
  Remote attachment URLs use the owning gateway's existing media proxy,
  including the owning session; no direct client-origin fetch is introduced.
- Raster decode checks dimensions and RGBA allocation before decoding, then
  applies 8192-pixel/64 MiB decoder limits. Source transfer is capped at 64 MiB.
  A refused decode keeps a successfully acquired original available for Save.
- SVG renders through the pinned resvg 0.45.1 into a bounded RGBA bitmap.
  Filesystem/URL image href resolution is disabled. Embedded raster dimensions
  are checked before resvg decode; nested SVG cannot load image resources.
  SVG source limit is 8 MiB, embedded bitmap limit is 16 MiB, total retained
  embedded decode budget is 64 MiB with 64 resources maximum, and compressed SVG
  is explicitly refused. Nested SVG is charged for encoded parse/tree expansion. This changes the decoder policy, not the original.

`preview::image::rasterize_svg` is reusable by thumbnail workers. Retained
Files/Artifacts/chat code paths must migrate to this helper before their SVG
entry points can share the same external-resource policy; passing raw SVG to
Rune ImageCache uses its upstream default resource resolver.

## Remaining runtime-dependent previews

PDF, audio/video and interactive HTML are not intercepted as implemented
previews until their actual runtime is wired. Existing source/download/native
open actions remain the fallback. A placeholder or external viewer is not
counted as inline player parity.

PDF needs an explicit per-platform PDFium binary in the application bundle,
with pinned upstream revision/build hash and notices. It must load from a
fixed bundle path, not current working directory or an arbitrary system
library. Page rendering must be bounded and serialized, with cancellation and
private document lifetime. Native page navigation/zoom/text extraction is a
useful increment; Chromium's search/selection/outline/print/forms remain
separate behavior until actually ported.

The existing rodio 0.21.1/Symphonia dependency already compiles common audio
decoders into the application (WAV, FLAC, MP3, Vorbis, MP4/AAC). A native audio
player can reuse that decoder without a new external runtime. Opus support is
not established by that feature set. Video on Linux/Windows cannot use Rune's
macOS-only AVFoundation player; it needs a packaged decoder. The proposed
GStreamer route must pin the 0.21 Rust family to match the existing GLib 0.18
link graph, and ship a declared private native runtime/plugin set with notices.
A developer machine's ffmpeg/pdftoppm installation is not a distribution plan.

Interactive HTML must execute inside an unprivileged opaque guest with no SDK
bridge, automatic navigation/popups or automatic OS opening. A real isolated
popout is possible with a separate Wry host; inline geometry, clipping, focus,
z-order and Linux Wayland/GTK event-loop interop still need a safe Rune host
seam. A raster screenshot does not implement HTML interaction.

## Verification

Targeted contracts live in `preview::tests` and `gateway::stream::tests`:
owner retirement/private cleanup, stale receipts/dialog IDs, pre-decode bitmap
limits, denied SVG local resources, bounded embedded rasters, image camera
anchor math, raw scoped Range/HEAD, conditional responses, transfer bounds,
redirect refusal and cancellation while the response body is stalled.

`xvfb-run -a node apps/rune/scripts/check-preview.mjs` launches a real Rune
window against a private synthetic peer, applies actual wheel / Ctrl-wheel
pinch / drag input, changes appearance, saves a byte-exact original and retires
its owner. It asserts zero agent RPCs and exactly one scoped image read.
Screenshots are direct native-window captures. This does not establish macOS,
Windows, Wayland, physical audio output or platform accessibility acceptance.

2026-10-29 native acceptance passed against the frozen third-checkpoint binary:

```
xvfb-run -a node apps/rune/scripts/check-preview.mjs \
  --binary /tmp/hermes-rune-third-acceptance/hermes-rune \
  --out /tmp/hermes-preview-proof-third
```

Actual wheel, Ctrl-wheel pinch and drag changed the native camera; the light
and dark captures were inspected at 73% fit and 137% zoom/pan. The saved PNG
was byte-identical to the source, owner retirement closed the preview, the
controlled peer received one scoped image read and zero agent RPC calls.
The three direct 960×720 window captures include an explicit test-only
lifecycle toolbar; they are acceptance fixtures rather than a product-screen
layout reference. The shared production view contains only its native preview
controls. No physical audio, microphone, provider or live agent was used.
