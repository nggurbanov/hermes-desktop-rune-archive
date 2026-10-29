# Native audio-file previews

This increment uses the native Rodio/CPAL stack, Symphonia container decoding,
existing bundled libopus and statically built Sonic. It does not launch a web
player, system ffmpeg, or an external viewer. Files, Artifacts and transcript
file actions route into the same owner-bound native preview controller.

## Behavior and resource ownership

Opening a player reads metadata on its owned worker and starts paused. No audio
output stream is created until explicit Play. Controls expose position/duration,
seek when actually supported, Play/Pause, volume/mute, pitch-preserving rate
0.25–4 and Retry. A source's connection/profile/session and original target are
captured before asynchronous work. Closing its owner/profile/connection cancels
its current network read, clears PCM and drops its output stream independently
of the currently selected chat.

The authenticated source uses the gateway's raw media byte stream, fresh tickets
and verified representation/range negotiation. It retains at most eight 256 KiB
ranges. Reliable random access needs a strong ETag and an actual accepted range;
otherwise the transport uses a single bounded sequential response and reports
seeking unavailable. A file may exceed a gigabyte; image preview's 64 MiB source
limit is not an audio-file size limit. Network/decode/Sonic run on the worker;
the Rodio iterator consumes only the bounded prepared PCM ring and emits silence
on underrun without advancing the media position.

A seek or seekable rate change clears/pauses queued PCM immediately, cancels the
old in-flight read and increments a playback generation. The worker opens a
fresh source/decoder and seeks the latest coalesced request while retaining
paused state. Metadata, PCM, progress and errors from the old generation cannot
commit. Sequential sources can change Sonic rate on their existing byte stream;
they cannot gain an invented seek capability. The audible position is derived
from consumed samples and each tempo epoch, not wall-clock time. Sonic's short
lookahead is the bound on a dynamic sequential-rate clock approximation. EOF
reconciles the known source duration, and a declared duration with missing audio
frames is an error rather than successful completion.

Rate is independent of model settings and agent state. Sonic pitch and sample
rate remain 1; Rodio Sink speed is never changed. Save original makes its own
captured-owner raw download, with progress and owner cancellation, into a private
0600 sibling temporary file. Only a complete successful file atomically replaces
the chosen destination. Playback-cache eviction never changes saved bytes.

Paused workers wait on a condition variable. Playing snapshots advance at most
20 Hz; hidden previews update their cached position without waking the entire
application for each progress sample. State changes and actions still wake it.
There are currently at most eight simultaneously open player controllers per
MediaPlayers owner; this is an explicit resource limit, not a claim of unlimited
concurrent-window media acceptance.

## Codecs and provenance

Symphonia is the exact existing locked 0.5.5 version, now a direct dependency with
AAC/FLAC/MP3/Vorbis/PCM and MP4/Ogg/MKV/WAV features. Ogg Opus uses the existing
libopus decoder with Symphonia's demux/seek API. Its narrow mapping-family-0
mono/stereo bridge handles the identification gain, header pre-skip, final
packet trim and 80 ms seek preroll. The Ogg mapper reports raw pre-skip-inclusive
frame timestamps; the bridge translates those into audible positions/duration.
Multichannel mapping families, chained streams and Opus in MP4/MKV are not yet
accepted; actual source magic identifies Ogg before enabling this bridge.

Sonic's unmodified sonic.c, sonic.h and Apache-2.0 license are vendored under
`vendor/sonic/`, pinned to `b93885dcb70aae50c6f76b0fe4e0868f029a077e`.
Its manifest records exact source hashes. The existing locked cc 1.10.0 builds
only the ordinary non-spectrogram library statically; runtime/build steps never
download dependencies or require Sonic/FFTW executables.

The compressed test assets contain mathematical 220/330 Hz stereo sine tones.
`assets/test-audio/manifest.json` records source parameters, encoder version,
exact offline generation commands and file hashes. ffmpeg is used only by the
explicit fixture-regeneration script, never by playback or ordinary tests.
Actual Ogg Opus test fixtures are generated in Rust with the locked libopus
encoder, valid Ogg CRC/pages, nonzero pre-skip and end padding.

## Verification and remaining acceptance

Focused tests exercise actual WAV decoding/seek, Sonic stereo frequency and
output duration at 0.5/1/1.5/2/4x, real Ogg Opus demux/pre-skip/end-trim/gain/seek,
compressed MP3/Vorbis/FLAC/AAC files, malformed allocation-bearing headers,
credential-free external metadata/original saving, fixed media clock during pause/underrun,
metadata without autoplay, cancellation and exact original saving. Final run
results are reported by the integration checkpoint; this document alone is not
acceptance evidence.

The final focused run passed all **16** audio/resource contracts, zero failed,
1.21 seconds (`/tmp/hermes-native-audio-patched-final-tests.log`). The final strict
private output run passed in 8.38 seconds
(`/tmp/hermes-native-audio-patched-final-private.log`), including actual
2x/play/pause/seek and stalled authenticated HTTP seek cancellation. Its monitor
captured 32,616 samples at 16 kHz, peak 6,975, with 220 Hz preserved instead of
440 Hz. These are local source acceptance results; parent native UI/binary
acceptance remains a separate checkpoint.

The existing explicit private Pulse/ALSA ignored test also opens the actual
MediaPlayers output, captures it through a null-sink monitor, and exercises 2x
play/pause/seek/EOF while checking 220 Hz remains 220 Hz rather than 440 Hz. Its
additional fake authenticated gateway stalls a later range body, then verifies
an active seek cancels that socket, clears old PCM and resumes at the latest
position over a fresh owned source. No default test uses physical output or
microphones, a user Pulse server, a real agent or a provider.

HTTP/HTTPS sources use a separate credential-free range/stream transport. Neither
metadata, seeks nor original downloads construct a Gateway, add its token/profile/
session query, send its ticket/cookies or follow redirects. Saving owns a fresh
cancellation token so dropping its temporary range reader cannot retire playback.

The four exact Symphonia 0.5.5 crates under `vendor/` carry reviewed local patches,
full MPL-2.0 notices, original registry archive checksums and per-file before/after
hashes. They bound metadata/packet allocations before reserving, reject sample
table counts that exceed their actual MP4 atom payload, use fallible reservation,
and reject invalid RIFF alignment, atom lengths and zero MP4 timebases. A shared
per-operation source-read quota bounds aggregate tiny metadata too: 64 MiB for
initial probe/seek, 32 MiB for the next decoded batch, plus 65,536 underlying
read calls per operation to bound non-progressing malformed readers. Empty coded
packets at truncated WAV EOF are rejected rather than looping forever. Quotas reset per operation;
seek offsets and total source size do not consume them. Patches affect decoding
only; no build mutates Cargo's global cache or downloads replacement sources.

Data-URL playback beyond already-decoded inline sources, cross-platform native devices, music-quality
listening, complete multichannel/chained Opus, persistent video rate and embedded
video/PDF/HTML are separate remaining work. Saved positions across transcript
remounts and simultaneous inline playback surfaces also remain distinct from
this shared explicit preview. This increment does not complete media parity.

2026-10-29 parent native acceptance: immutable binary
`02bca4b8963a81fa79c1b59b57dbd5216f531f7f7e76520c94d36a7f70829d77`
passed `check-audio-preview.mjs` in an actual Rune window: paused metadata,
pointer and physical-keyboard seeking, rate, mute, volume, both appearances,
1024 × 520 geometry, byte-exact original save and owner retirement. Evidence is
in `/tmp/hermes-goal-five-audio`. This proof uses the production controls with
a private loopback peer; Play/output is established by the separate strict
private-device case, not by a native Play click. Three real application-controller
tests cover routing/owner seams; the ordinary Viewer render/input acceptance is
recorded below. Closing a rendered preview is also fenced by its captured PlayerId.

The same immutable binary subsequently passed all **11** normal Viewer cases in
`scripts/check-audio-navigation.mjs`: Files to paused audio in both themes and a
1024 × 520 viewport, close back to the source chat, replacing one file player with
another, transcript file actions and return to their source message, indexing
an audio delivery from another profile, opening it with that exact profile/session,
restoring the existing Artifacts selection after Close, and retiring the player
when the connection incarnation changes. The fixture rejects every HTTP mutation
and any RPC outside its explicit read/attachment allowlist. The cross-profile
recording is read as `archive / archive-audio` while the window remains on
`research`; neither the ambient profile nor the selected chat replaces its owner.
Evidence: `/tmp/hermes-full-audio-navigation/{result.json,requests.json}` and the
11 PNG/tree/log triples. This uses the ordinary Viewer and its production input
callbacks through Rune's offscreen renderer with actual asynchronous loopback
HTTP/WebSocket workers. It does not establish native window-manager behavior,
click Play, open physical output, or connect to an installed agent/provider.

Run with `node apps/rune/scripts/check-audio-navigation.mjs --binary PATH --out DIR`.


The sixth preference integration reran all eleven full Viewer audio-navigation
cases successfully on immutable binary `1ccda04a1714fb312aeb4fa4f404e6f7c714bbe609517368f22248a2083896dc`.
Evidence is in `/tmp/hermes-goal-six-audio-navigation`; source metadata/output
limits remain as described above. No audio player or transport production code
changed after the fifth private output proof.
