# Native voice implementation and verification

Updated: 2026-10-29. Retained Hermes Desktop source remains the behavior reference.
Rune is pinned at `bfb8facae2e4665e5e6d17afe85561884a7fd402`.

## Implemented paths

- **Dictation:** actual CPAL capture, elapsed time/input meter, manual stop,
  signed mono PCM, live STT partials where supported, WAV upload fallback, and
  transcript insertion into the captured draft. Dictation does not submit a turn.
- **Speech-to-text + Hermes voice:** VAD, STT, a canonical durable GUI submission
  carrying `voice_turn`, confirmed matching completion, real TTS playback, then
  listening again. Partial or unrelated replies never enter chained TTS.
- **GPT-Live:** actual SDP/ICE/DTLS/SRTP/SCTP, Opus audio and vendor data channel.
  Delegations submit real Hermes turns with separate spoken context; a newer
  delegation interrupts the older turn. Complete sentences stream as commentary,
  active tools send thinking appends, and completion flushes the unspoken tail.
- **Engine choice:** profile-scoped `config.set` plus authoritative status
  readback. An active call keeps its actual engine; the confirmed preference
  applies to the next call.
- **Speaker/ear controls:** automatic reading of completed replies and separate
  wake listening. The current native wake dispatcher recognizes the built-in
  English/Russian Hermes aliases; arbitrary configured wake phrases and stop-word
  customization still need runtime integration before claiming exact parity.

The native composer exposes dictation, the voice primary, engine picker, mute,
finish-speaking and end controls. Audio is cancelled when its owning
connection/profile/chat is left. Draft adoption follows the existing task
identity into the stored chat without aliasing a future new draft.

## Linux media engine

The actual WebKitGTK 2.52.6/Xvfb host exposed a secure origin and
`getUserMedia`, but neither standard nor prefixed `RTCPeerConnection` was
available, even with media/WebRTC settings enabled and GStreamer plugins
installed. Linux therefore uses the native Rust WebRTC implementation, not an
STT alias or an external browser process.

`audio/native_live.rs` provides the native peer, RTP feedback, Opus encoding,
decoding, loss concealment, bounded playback buffering and native devices.
`audio/dsp.rs` uses the real WebRTC AudioProcessing AEC3 with automatic delay
estimation, a high-pass filter, moderate noise suppression and digital AGC2.
The render reference is taken from the samples requested by the output mixer,
including silence, rather than decoded packets waiting in a queue. Capture and
render are processed in corresponding 10ms frames. The microphone is not muted
merely because the remote voice is speaking.

Microphone cancellation is independent of a pending gateway request. Muting
clears buffered microphone audio, sends silence on the RTP track and sends the
vendor mute event. Start is not reported as listening until capture is open and
the vendor session has actually started. Failed/closed media/control channels
surface errors and release owned devices.

## Transport and admission ownership

All HTTP/WS operations retain the captured connection and profile. Native local
requests use the existing private gateway ticket path; tokens do not enter the
WebView, SDP, persisted journal or executable arguments. STT/TTS leases release
the same owning identity. Streaming transcription sends explicit mono sample
rate, PCM and end-of-stream. TTS may fall back only before any audio has played.

GPT-Live uses the existing `/api/audio/voice-live/session` exchange with byte-exact
SDP. The common native/WebView protocol retains delegation IDs, recent spoken
context, user-versus-assistant transcript ownership, mute/close controls, bounded
commentary chunks and deduplicated vendor tasks. Speech normalization preserves
prose and table headers while silencing code, URLs and dense machine identifiers
without inserting English placeholder text.

GUI creation, durable submission IDs, queued admissions, approval/model guards,
stream replay and stop use the canonical conversation pipeline. Commentary
progress is gated by the active input/admission/generation; completion is gated
by the matching completed input. Background or interrupted turns cannot speak
into the newer voice request.

## Verified evidence

`cargo test --locked --manifest-path apps/rune/Cargo.toml audio:: -- --nocapture`
passed **15 tests**, with the explicit device acceptance test ignored by default:

- Synthetic HTTP/WS servers verified owner/profile routing, lease release,
  byte-exact SDP, PCM/end-of-stream and partial-versus-final transcription.
- Two production native peers negotiated real loopback ICE/DTLS/SRTP/SCTP,
  exchanged a delegation/commentary over the real data channel, and transported
  and decoded actual Opus RTP. No browser, audio device, provider or agent was used.
- The real AEC3 processed a synthetic 16-second room/double-talk signal. Measured
  echo attenuation was **73.87dB**; near-end correlation during simultaneous
  playback was **0.980**, allowing at most 30ms processing-delay alignment.
  This also checks that echo reduction is not achieved by half-duplex mic gating.
- State tests cover draft adoption, stale task rejection, engine changes during
  a live recording, failed initialization and commentary deduplication.

The explicit native device acceptance command also passed:

```sh
HERMES_RUNE_VIRTUAL_AUDIO=1 cargo test --locked \
  --manifest-path apps/rune/Cargo.toml -- \
  audio::device::virtual_tests::native_virtual_device_roundtrip \
  --exact --ignored --nocapture --test-threads=1
```

That test owns an empty private Pulse daemon with only a null sink, a private
runtime/socket directory and an isolated ALSA child process. It never uses the
existing user Pulse server or loads hardware modules. Actual Rodio playback was
captured through actual CPAL: **17,415 samples at 16kHz**, peak **15,284**, mean
power **0.05785**. The owned daemon and temporary files were cleaned up.

## Limits of the evidence

No real agent prompt, provider SDP exchange, external transcription/TTS call or
human microphone was used for these checks. Provider interoperability, physical
microphone/speaker echo behavior, permissions and device switching still require
acceptance on the platform and hardware being claimed.

macOS/Windows use the native Wry WebView media path with real
`getUserMedia`/`RTCPeerConnection`; that implementation has not been run on either
platform in this Linux environment. Linux evidence does not establish their
media or permission behavior. Synthetic scenes remain useful for geometry and
state screenshots but are not substitutes for media acceptance.
