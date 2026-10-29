# Native media player plan

2026-10-29. Read-only implementation audit and proposed next increment. This is
not a statement that audio/PDF/video players have shipped. No dependency,
production/test source or runtime binary changed during this audit.

## Original behavior to preserve

The retained Desktop renders actual audio/video controls, not an external-viewer
button disguised as a player. Audio preloads metadata; transcript video defaults
to no preload and keeps native controls/inline playback. File reads bind the
transcript's connection/profile and stored-or-runtime session, including sandbox
paths. Expired stream tickets are renewed with bounded retries and paused/time
state restored. Source identity changes do not briefly paint the previous file.
See [playback resolution](../../desktop/src/hooks/use-media-playback-src.ts),
[media paths](../../desktop/src/lib/media.ts), and
[attachment controls](../../desktop/src/components/assistant-ui/markdown-text.tsx).

Video remembers the last 24 source positions in process memory across virtual
remounts; device-level rate persists and propagates across windows. Its accepted
range is 0.25–4, with 1 as default. Audio uses the browser's audio controls, so a
new shared audio-rate preference would be a deliberate extension, not an existing
persisted-audio contract. See [video lifetime](../../desktop/src/components/chat/transcript-video.tsx)
and [rate storage](../../desktop/src/store/video-playback-speed.ts).

Keep explicit Play, Pause, position/duration, Seek where supported, volume/mute,
rate with preserved pitch, loading/buffering/ended/error, retry, and original
save/open actions. Opening a preview or restoring a row must not open a device
or autoplay a new source. A foreground chat change must not reassign its owner.

## Audio first: concrete dependency choice

Use the already-locked Rodio 0.21.1/CPAL 0.16 output stack, but move file decoding
and tempo work onto an owned media worker. The existing completion-sound actor
is a cue/preset API with detached sinks; it is not a reusable long-file/seek
controller. Existing dictation/TTS remains separate. A future shared device mixer
can be extracted if necessary; this increment need not change cue arbitration.

The lock already contains Symphonia 0.5.5 and its WAV/PCM, FLAC, MP3, Vorbis, AAC,
MP4, Ogg and MKV demux/decoder packages. Add an exact direct Symphonia dependency
with an explicit feature set to use its packet/seek API; this reuses the existing
resolved libraries. Its `MediaSource` accepts Read+Seek+Send+Sync with separately
reported seekability and byte length. The implementation must advertise the
capability actually available from the source, not assume every HTTP URL seeks.
[Symphonia 0.5.5 primary documentation](https://docs.rs/symphonia/0.5.5/symphonia/).

There is no independent pitch-preserving tempo processor in the present lock.
Rodio `Sink::set_speed` changes pitch, so it cannot implement playbackRate parity.
[Rodio speed semantics](https://docs.rs/rodio/0.21.1/rodio/struct.Sink.html#method.set_speed).

Preferred addition: vendor unchanged Sonic `sonic.c`, `sonic.h` and LICENSE from
commit `b93885dcb70aae50c6f76b0fe4e0868f029a077e` of waywardgeek/sonic.
It is Apache-2.0, ordinary ANSI C; the non-spectrogram implementation needs only
standard C headers. Compile it statically with exact build dependency `cc =
"=1.10.0"`, already resolved transitively. Do not build the CLI, experimental
variant, spectrogram/FFTW or download anything at build time. Preserve its
license/copyright and record modifications if a patch becomes necessary.
[Sonic source](https://raw.githubusercontent.com/waywardgeek/sonic/b93885dcb70aae50c6f76b0fe4e0868f029a077e/sonic.c),
[build recipe](https://raw.githubusercontent.com/waywardgeek/sonic/b93885dcb70aae50c6f76b0fe4e0868f029a077e/Makefile),
[license](https://raw.githubusercontent.com/waywardgeek/sonic/b93885dcb70aae50c6f76b0fe4e0868f029a077e/LICENSE).

Verified source SHA-256 values, read into memory without adding vendor files:

| File | SHA-256 |
| --- | --- |
| sonic.c | `4a21d8086f844e3e68cb4f85961623a69d35da8ca93cf6ea704ccd125e64eaf5` |
| sonic.h | `a2fc087b68c25141e2fb7ab56e71b111bee79cf311fcc850d492eb574c0eeffe` |
| LICENSE | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` |

Use Sonic's streaming Float input/output, separate speed/pitch/rate controls,
and EOS flush. Keep pitch=1, sample-rate ratio=1 and Rodio sink speed=1; only the
Sonic speed changes. Bound and drain each write, handle allocation failure, clamp
supported sample rates/channels, and keep its pointer owned by one worker.
[Sonic API](https://raw.githubusercontent.com/waywardgeek/sonic/b93885dcb70aae50c6f76b0fe4e0868f029a077e/sonic.h).
This is a proposed pitch-preserving pipeline; tone/duration and real output tests
must establish its actual behavior and quality before enabling the rate control.

### Ogg Opus without another container library

Symphonia's locked Ogg mapper already recognizes OpusHead, emits
`CODEC_TYPE_OPUS`, provides header bytes, pre-skip/delay and a 48 kHz timebase;
its default decoder registry has no Opus decoder. Existing `opus = 0.4.0`
resolves to opusic-sys 0.7.6, whose default bundled build statically compiles
libopus (its bundled source identifies 1.6.1). No libopusfile/ogg system install
is necessary. Evidence is the exact Cargo.lock plus the locked crate sources
`opusic-sys/build.rs`, `symphonia-format-ogg/src/mappings/opus.rs` and
`rodio/src/decoder/symphonia.rs`.

Add a narrow Symphonia Decoder adapter over existing `opus::Decoder`/`MSDecoder`,
using the existing Ogg demuxer and seek index rather than writing an Ogg parser.
Handle OpusHead gain/channel mapping, pre-skip, packet trim_start/trim_end, reset
and accurate seek preroll. Ogg/Opus seek needs at least the specified 80 ms preroll;
mono/stereo should be the first verified matrix, then mapping-family-1 multistream.
Do not claim multichannel/chained-file parity until those cases pass.
[Ogg Opus specification](https://www.rfc-editor.org/rfc/rfc7845.html).

## Authenticated range/cache contract

Reuse the actual [raw transport](../src/gateway/stream.rs), not base64 image
endpoints. Its request limit is caller-selected; image preview's 64 MiB cap must
never become the maximum playable audio-file size. The source can be gigabytes
while only a bounded sliding set of byte ranges is cached.

Proposed transport addition, not yet an implemented API:

```
stream_file_events(profile, request, cancel, on_headers, output, on_progress)
```

The current helper returns metadata only after writing the body. A range cache
needs a pre-body header callback to reject a wrong 200/206/Content-Range/validator
before writing at a requested offset. Preserve the existing wrapper as a caller.
Use exact gateway/profile/session ownership and fresh native HTTP tickets per
request. Reuse one read-only Gateway per actor rather than reconnecting a
WebSocket for every chunk. Never follow redirects with gateway credentials.

Suggested initial resource policy:

- 256 KiB–1 MiB blocks; at most two fetches, one forward prefetch plus one seek.
- Private 0700 cache directory/0600 block files, 64 MiB disk LRU and a small
  2–4 MiB read window. This limits retained ranges, not total source size.
- A bounded PCM ring (about one second) and fixed Sonic input/output batches.
  Network and decode never run in the CPAL callback or Rune frame.
- Cache key includes full connection epoch/revision, profile, session, source
  and representation validator. Strong ETag + If-Range is preferred. Verify
  Content-Range bounds/total and exact body length; do not merge changed ETags.
- If a nonzero Range returns 200, do not place its bytes at the requested offset.
  If range/validators are unavailable, expose sequential buffering/limited seek
  honestly. A progressive spool needs an explicit disk quota and actual states;
  it must not falsely become a random-access source or allocate the entire file.
- No strong validator means separate HTTP responses are not proven to be the
  same file. Prefer a single sequential representation or require reload when
  consistency cannot be established. Last-Modified alone is weaker evidence.
- Closing or retiring a source cancels all requests and wakes blocked readers.
  Cancellation must also interrupt a stalled body; the existing async transport
  already tests that behavior. Pending seek/rate controls are coalesced and carry
  a new decoder generation, including PCM blocks and late replies.
- Original Save streams to the chosen destination atomically, with progress and
  cancellation; it is independent of preview-cache eviction and image limits.

A custom Read/Seek MediaSource may wait for a cache miss on the decode worker.
The playback callback only consumes already prepared samples. On underrun it
outputs silence, reports Buffering and does not advance the audible media clock.
A seek cancels stale work, clears old PCM, resets demux/decoder/Sonic, decodes
preroll, discards samples before the target and then resumes the previous paused
state. A rate change records a tempo epoch; time comes from consumed media
samples and their epoch, not wall-clock elapsed or raw Sink::get_pos. Bound any
clock approximation to Sonic's lookahead and reconcile the exact EOS duration.

External HTTP/data sources are a separate baseline path: Desktop passes them
through directly. Native support must use a credential-free HTTP client and an
explicit source policy, with no gateway token/ticket/header copied to that URL.
The current `/api/media/proxy` JSON route is a bounded image path, not a proven
raw streaming audio API. Do not invent such a backend route or silently use
base64 for large external clips. Agent-file streaming is the first vertical.

## Proposed feature API and first acceptance gate

New feature-owned `media_player` state can be shared by Files, Artifacts, chat and
spectator views; retain the existing image service separately.

```
open(connection, Target { owner, source, label, kind: Audio }) -> PlayerId
act(id, Play | Pause | Seek(seconds) | Rate(value) | Volume(value) | Mute | Retry)
poll() -> changed
close_owner / close_profile / close_connection
```

The snapshot exposes Paused/LoadingMetadata/Buffering/Playing/Ended/Error,
optional duration, actual position, seekability, buffered ranges, volume/mute,
rate and a useful failure. No audio device opens until Play. Only visible
progress needs periodic UI wakeups; idle/paused sources do not run a timer.
Remember positions in a bounded owner-scoped map across row remounts. Preserve
video's existing device preference; do not confuse it with model/agent state.

Required controlled proofs before an audio capability becomes available:

1. Generated PCM WAV, MP3/Vorbis and Opus fixtures with recorded generator/hash;
   production playback must not require ffmpeg. Opus includes nonzero pre-skip,
   end trim/gain, seek preroll and malformed headers, not only raw Opus packets.
2. A synthetic 220 Hz tone/two-tone and stereo fixture: verify source duration vs
   output-frame count at 0.5/1/1.5/2/4x, spectral peak stays at source pitch,
   channels stay aligned, EOS drains exactly once and seek discards old samples.
3. A fake scoped gateway advertises a sparse file larger than 64 MiB (preferably
   >1 GiB); seeking near its end reads only bounded ranges. Cover ignored Range,
   wrong ranges/length, ETag change, token expiry/renewal, redirect refusal,
   stalled reads, cancellation and owner/credential replacement.
4. Actual Rodio/CPAL output through the existing strict isolated file/null ALSA
   harness, never physical speakers or microphone. Capture samples to verify
   pitch/duration/pause/seek rather than testing a fake player. Native Rune
   controls/screenshots must exercise this pipeline. Cross-platform output and
   music-quality checks remain explicit acceptance work.

## PDF: pinned runtime, serialized progressive rendering

The concrete non-V8 PDFium candidate is immutable release `chromium/8125`
(158.0.8125.0), build-repository commit
`4bdb7acd45b39ef6a7fc2a5b4c83f0398e45c9a6`. The release API supplies the
following published SHA-256 digests. No binary was downloaded or locally
validated in this audit.
[Immutable release](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium/8125).

| Asset | Published SHA-256 |
| --- | --- |
| pdfium-linux-x64.tgz | `e1c3ce6e270cb5e0c20816b379ad9f2c1309972697c505f508de92bf5e7d6659` |
| pdfium-linux-arm64.tgz | `8b8818f2ec57d1b213d952ca6d7f0ffeca9d0110487d0e20ad83ba918bc92efa` |
| pdfium-mac-x64.tgz | `8a5193535b80c7215d8932f6647ae5097465b6dba2811602c21e120c2af112b0` |
| pdfium-mac-arm64.tgz | `4939bd935ac83ddc09a3a4f038bc43a19cea1660316a3667f26cb07a070e139d` |
| pdfium-win-x64.tgz | `9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b` |
| pdfium-win-arm64.tgz | `d394ba61991fb53f4d87f70988035f337d9e73f2bd2c3c1b966df8c43fcabb05` |

Important API pairing: current pdfium-render 0.9.4 defaults to the **7881** API,
not 8125. Do not assume that pairing works because both say latest. Prefer a
small isolated renderer using pinned 8125 public headers and narrow RAII FFI
loaded via already-locked libloading 0.8.9, or validate/update the wrapper against
8125 before choosing it. An older matching runtime is an alternative requiring
an explicit version decision, not a silent fallback.
[Wrapper feature definitions](https://raw.githubusercontent.com/ajrcarey/pdfium-render/master/Cargo.toml).

Load only from the fixed app bundle runtime path with verified manifest hash,
not current working directory/system-library fallback. Package the runtime,
PDFium license and its actual third-party notices on each OS; validate target
ABI/minimum OS and signing/loader behavior in packaging acceptance. A private
renderer process can own one document, serialize PDFium calls and receive only
private source descriptors, not gateway credentials. Bound bitmap dimensions,
page/result queues and retained original/cache lifetime.

Use real `FPDF_RenderPageBitmap_Start`, pause callback, Continue and Close.
Close must run after completion or cancellation; a helper process provides a
hard cancellation/crash boundary when parsing does not yield to progressive
rendering. Native page/zoom/pan/text controls are distinct from rendering pixels.
[PDFium progressive API](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/public/fpdf_progressive.h).
First prove two pages, text, zoom/pan/save and cancellation with a controlled
fixture. Search/selection/outline/print/forms are still Chromium PDF-viewer
contracts to port; a rendered page does not establish them.

## Video: runtime/process choice remains a separate increment

Pinned Rune has a real AVFoundation player on macOS; its Linux/Windows player
backend is unsupported. A desktop video viewer therefore needs real decoding,
clock/seek and audio synchronization on those targets, not animated images or
an automatic external-app launch.

Two concrete GStreamer choices:

- In the current executable graph use gstreamer-rs **0.21.3**, which declares
  GLib **0.18**, matching GTK/Wry's present GLib link family. Its v1_24 API subset
  can use an explicitly packaged compatible 1.x runtime; verify that exact pair.
  [0.21.3 manifest](https://raw.githubusercontent.com/GStreamer/gstreamer-rs/0.21.3/gstreamer/Cargo.toml).
- Preferred isolation option: a small media helper with its own Cargo graph and
  lock, gstreamer-rs **0.25.4**, private GStreamer **1.28.8** and a declared plugin
  manifest. The current Rust family needs GLib **0.22**, so do not add it to the
  GTK 0.18 graph or try to solve the conflict with features. Independent process
  loading also separates bundled native libraries/plugin discovery from Wry.
  [0.25.4 manifest](https://raw.githubusercontent.com/GStreamer/gstreamer-rs/0.25.4/Cargo.toml),
  [1.28.8 release](https://gstreamer.freedesktop.org/releases/1.28/).

For either choice, ship actual private runtime libraries, selected demux/decoder
plugins, scanner, registry policy and notices; do not assume distro plugins or a
system ffmpeg install. GStreamer supports private deployment and Cerbero builds.
[Deployment documentation](https://gstreamer.freedesktop.org/documentation/deploying/index.html).
A dedicated FFmpeg-library helper is another packaging option, but introduces
another codec/runtime build and has no existing locked binding here; it is not
an already available lightweight dependency.

Feed only authenticated owned bytes/range requests to the helper; no gateway
credential in argv, environment, logs or a codec-owned URL. Decode to a bounded
latest-frame BGRA shared-memory mailbox and Rune NativeSurface, with PTS/generation
fences and the actual media clock. Avoid shipping irrelevant network/HLS/RTSP
plugins that could turn a data-only decoder into an independent fetcher. Initial
SDR formats/resolutions, full-screen/PiP/subtitles and platform codec coverage
need a declared acceptance matrix. Prove actual WebM/MP4 decoding, seek/rate,
audio/video sync, EOF and detach using private fake peers and silent output.
Interactive HTML remains its separate isolated-guest integration; PDF/video
work does not resolve that browser-host seam.
