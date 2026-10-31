# Native audio-file previews

This increment uses the native Rodio/CPAL stack, Symphonia container decoding,
existing bundled libopus and statically built Sonic. It does not launch a web
player, system ffmpeg, or an external viewer. Files, Artifacts and transcript
file actions share the owner-bound native player. Transcript file attachments
also mount independent inline controls for FLAC, M4A, MP3, OGG, OPUS and WAV.

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
The eight-player preview admission limit does not evict mounted transcript
players. Each inline player is owned by its mounted transcript node: overscan
retains it, removal cancels it, and a remount receives a fresh player ID. This
does not establish unlimited concurrent-window media acceptance.

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

## Transcript FILE slice

The Linux Runtime tests cover independent controls using real loopback WAV input
with test-only null output, rerender retention, offscreen overscan, retirement
and fresh remount. Separately, `/tmp/hermes-file-audio-accept-1793379237-367657/REPORT.md`
and run5 case summaries record bounded actual Linux native acceptance on the sealed
91f8 ELF (`91f809abd6bdd82f8e9951b47dce4ad396ef2774417cf9b0d9d3412357f1506b`):
Light and Dark each showed two independent inline FILE players for the same WAV
path. First play advanced while the second stayed paused; second play, first pause,
+5 s seek, 100% to 90% volume and search rerender state retention passed. The
actual app PID produced uncorked PCM at a private PulseAudio null sink through
private ALSA routing. The fixture checked source, profile, session and ticket on
HEAD and ranged GET. The 1.5x control was clicked, but its effect was unverified.

Runs 3 and 5 passed; runs 1, 2 and 4 retained harness failures. Five frozen
hashes matched before and after; recorded owned children exited and resources were
released, but the transient descendant census was incomplete. Native cull/remount,
owner change, errors/ticket refresh, keyboard controls, other codecs and other OSes
were unrun in this acceptance. The WAV case does not prove all codecs. Local
Markdown link/image-form audio and MEDIA embeds were unimplemented in the 91f8
ELF; later source work is documented separately below. Broader remount/keyboard/native matrices and the codec and platform
limitations above remain open. This is not full audio parity.

### Local Markdown link implementation, 2026-10-30

The current source replaces authored absolute-path and supported `file:` Markdown
links for the six baseline audio extensions at their parsed occurrence. Distinct
occurrences have separate mounted controllers, also separate from structured FILE
parts. The player uses the existing scoped agent-file transport. Ordinary HTTP
links and code are not converted; table cells remain ordinary links. Search
excludes replaced labels and indexes the displayed filename. This is a narrow
Linux Runtime test slice, not native-window acceptance for the current source.
The 91f8 native WAV evidence above applies only to that frozen binary. Paragraph
row embedding does not establish complete inline typography parity. MEDIA and
`#media:` normalization, image-form audio, refresh, broader formats and other
operating systems remain pending. The separate original MEDIA normalizer oracle
is not a whole hydration/render or final UI proof.

Linux test evidence: `/tmp/hermes-markdown-audio-red.log` records the original
runtime failure (0 versus 2); `/tmp/hermes-markdown-audio-regressions.log`
records 8 focused audio tests passed, and `/tmp/hermes-markdown-audio-common.log`
records 887 passed and 14 ignored in the locked offline common gate. The loopback
fixture used null output and checked the captured profile, session and path; no
native-window acceptance is attributed to these tests.
The final focused search run passed 24 tests, including editor Ctrl+F, preedit,
owner isolation and mounted highlights (`/tmp/hermes-markdown-audio-search-final.log`).

After renderer review, empty authored audio labels also mount independent players.
The renderer preserves empty-link layout when no embed is returned and falls back
to visible text when copying a selection in a block with an embedded link. The
final app common gate passed 888 tests with 14 ignored
(`/tmp/hermes-markdown-review-common-final.log`); renderer copy and empty-link
RED/GREEN evidence is in `/tmp/hermes-markdown-review-red.log`,
`/tmp/hermes-markdown-review-callback-red.log`, and
`/tmp/hermes-markdown-review-embed-formatted.log`.

### Assistant MEDIA audio source slice, 2026-10-30

Assistant transcript and search now derive display Markdown from the accumulated
raw text, keeping history and operation inputs raw. Recognized audio MEDIA links
mount separate scoped inline players; HTTP(S) MEDIA targets use the existing
credential-free external range source, while ordinary external Markdown links
remain links. Non-audio MEDIA opening, image-form and table audio, and full
inline typography remain unsupported or unverified. This source has not had
native-window acceptance; the frozen 625c6cf5 binary predates this work.

The normalizer passed 34 original normalization cases, 11 accumulated-stream
cases, and seven additional oracle cases in one Rust test. The initial run exposed
three URL/ECMAScript-whitespace mismatches; the corrected run passed
(`/tmp/rune-media-red-20261030.log`, `/tmp/rune-media-green-20261030.log`).
Private Rune Runtime tests passed two MEDIA players plus structured FILE,
independent play, prefix retention, removal/remount and owner-scoped loopback;
search and a credential-free external ranged URL including its query passed
(`/tmp/rune-media-runtime-green-20261030.log`, `/tmp/rune-media-search-20261030.log`,
`/tmp/rune-media-external-20261030.log`). A temporary test-only event-queue hook
exercised `message.delta` through `Update::Changed` and search with raw text
retained; the hook and test were removed to preserve the requested source scope
(`/tmp/rune-media-stream-20261030.log`). Quote closure, path/classification
changes, backfill, scope fences, late events, native GPU rendering, other codecs
and other platforms remain unverified.

The one isolated common run had 891 passed, 14 ignored and one unrelated private
subagent-socket timeout (`/tmp/rune-media-common-20261030.log`); its exact isolated
recheck passed (`/tmp/rune-media-common-failure-recheck-20261030.log`). This is
not a green common gate or a full parity claim.

### MEDIA queue and activation follow-up, 2026-10-30

The current source retains a test-only event injection into the production
conversation reply queue. `queued_media_delta_polls_through_viewer_and_refreshes_mounted_search`
drives `Viewer::poll_conversations` and `Update::Changed`, checks raw accumulated
text after quote closure, and finds the mounted search highlight. The separate
`queued_quoted_media_closure_mounts_and_retains_the_player` uses a private
loopback peer and null output: an unclosed quoted directive has no player, the
closing quote mounts a play/pause control, and a subsequent delta retains the
same controller. These are Runtime tests, not native acceptance. Logs:
`/tmp/rune-media-event-new-open-20261030.log` and
`/tmp/rune-media-live-player-retention-20261030.log`. Failed intermediate event
runs remain at `/tmp/rune-media-event-new-recheck-20261030.log`,
`/tmp/rune-media-event-new-final-20261030.log` and
`/tmp/rune-media-event-new-fixed-20261030.log`; the first used a draft instead
of a stored-session scope and the latter had search closed. The initial isolated
HOME invocation lacked `RUSTUP_HOME` (`/tmp/rune-media-event-new-20261030.log`).

Non-audio generated `#media:` links now decode their target at the existing
owner-checked link callback, which routes local files through `open_chat_file`
and credential-free HTTP(S) links through the existing explicit URL action.
Actual non-audio activation has not yet been exercised by an input test.

The previous common failure was a fixture synchronization defect: its three-second
completion deadline was cut short by an unconditional unwrap after a 200 ms
receive timeout. The fixture now continues on that timeout while preserving the
original deadline and failing on disconnection. Its focused recheck passed
(`/tmp/rune-media-focused-actual_socket_spectator-20261030.log`). The final
locked/offline serial common gate passed 894 tests, zero failed, 14 ignored
(`/tmp/rune-media-common-final-20261030.log`); the earlier post-fix gate passed
893/14 (`/tmp/rune-media-common-new-20261030.log`). Focused normalizer, existing
MEDIA/FILE mount and search checks each passed one test in the corresponding
`/tmp/rune-media-focused-*-20261030.log` files. No native checkpoint is implied.

Still unproved: partial unquoted File-to-Audio classification, changed-path
retirement with a held old transfer and late completion, captured lease and
source/profile/editor-generation fences, backfill and live-to-durable receipt
identity/controller reconciliation, and actual non-audio link clicks. Image-form,
table audio, full inline typography, wider native rendering, codecs and other platforms
remain open; the later bounded native streaming case is recorded below. The final source-level gate does not establish those behaviors or
full functional parity. No complete transient process census was performed.


### External range decoder regression — source only, 2026-10-30

**UNCOMPILED / UNRUN:** `media_player::range_read_tests::external_range_wav_worker_decodes_to_end_with_original_query_and_no_credentials` defines a loopback strong-ETag range peer and uses the real player worker, external `RangeSource`, decoder budget, cancellation/generation and null output. Its test-only per-player packet-error capture preserves the bounded underlying `next_packet` debug error for a later run. No runtime result or production fix is claimed. The earlier frozen native run accepted the initial HEAD and 206, then reported the generic packet-read failure before a further observed request; the underlying cause remains unknown.

The copied synthetic 20 s WAV is `assets/test-audio/range-20s.wav`: SHA-256 `43cf157dfb1c200728404187fabbc396bae2b8de715348f3959314617d557c2b`, 3,840,044 bytes; RIFF has 16-byte `fmt ` and 3,840,000-byte `data` chunks. The fixture is synthetic (48 kHz stereo signed 16-bit, 220/330 Hz); PCM SHA-256 `c00d203c1ab29fab91e2ecfa6a12f60c8663e50ae3e22b951a675db5d90f97fb`. Later, after capacity and execution authorization, run from `apps/rune`: `cargo test --locked --offline media_player::range_read_tests::external_range_wav_worker_decodes_to_end_with_original_query_and_no_credentials -- --exact --nocapture`. Full Media, Create, Linux, macOS and Windows parity remains incomplete.


### Bounded native streaming and external range diagnosis, 2026-10-30

`/tmp/hermes-media-stream-closure-20261030/REPORT.md` and `cases.json` record
independent Light and Dark live agent-file cases on immutable 8c6e. An incomplete
quoted directive displayed zero players; its closing delta mounted one 20-second
player. Scoped playback produced app-PID PCM on private Pulse. The same full
Runtime player path, paused position and volume survived a later append and
search rerender; this does not independently establish worker generation or
object allocation identity. The exact Unicode filename had a visible highlight
and nonzero count in both themes; encoded `#media:%2Fprivate%2F` returned zero
in both. `Update::Changed` execution is inferred from frozen source and observed
transitions, not direct enum instrumentation. This supplements
`/tmp/hermes-media-native-execution-20261030/REPORT.md` and resolves filename
and alias for this live case, not every historical case. Ordinary HTTP negative,
wider ownership/cull, durable receipt, rate, refresh and other OSes remain
unproved. Marker, five artifacts and WAV hashes matched before and after; owned
processes exited and recorded resources were released, with no full transient
process census.

The external strong-ETag case remains **not accepted**. Initial 206 was accepted
and cached and the decoder opened; a later `FormatReader::next_packet` returned
a non-`UnexpectedEof` error, with its variant and message discarded at
`decode.rs:255–256`. Exact peer replay and next/final/validator controls passed,
but no subsequent native range was observed. Production versus fixture cause is
undetermined. `/tmp/hermes-range-diagnosis-20261030/REPORT.md`, `diagnosis.json`,
`replay.json` and `/tmp/hermes-client-boundary-20261030-956469/REPORT.md` and
`diagnosis.json` retain the evidence. gdb/lldb were absent; the boundary task
made no new native run. The earlier missing-ETag sequential/disabled-seek
fixture explanation is distinct from this decode failure. The source-only test
and bounded per-player/generation capture above have only static fmt/diff/hash
and cmp evidence, no compilation or test execution and no production fix.


### Later headless range execution (2026-10-30)

The registered 20-second WAV worker regression compiled and passed with null
output, checking the asset hash, strong validator, original query and absence of
credentials. It also passed in the 906-test common run. It did not reproduce the
frozen native packet failure, so its cause and native playback acceptance remain
open. Earlier source-only status describes the pre-execution state.

The later vendor/Create gate attempt did not change or retest audio. The 20-second null-output result above belongs to the previous app test ELF; subsequent test-source additions have not been built. The native packet failure remains unexplained and native playback acceptance remains open.

### Current RAM-backed test run (2026-10-30)

The current-source common test ELF passed the 20-second external range WAV
worker regression with null output, including query and credential assertions.
The native packet error remains unexplained; this result does not establish
native playback acceptance. The test ELF is volatile RAM output, separate from
the historical frozen checkpoint.
