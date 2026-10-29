# Completion sounds

Authority: retained `apps/desktop/src/lib/completion-sound.ts`,
`store/completion-sound.ts`, `store/haptics.ts`, `store/ambient.ts`,
`electron/event-dedupe.ts`, and the `message.complete` handler in
`app/session/hooks/use-message-stream/gateway-event/message-stream.ts`.

## Native implementation

- All fourteen original presets and their names, plus variant 0 (None), are
  available in native Notifications → Sounds. Default is variant 1; invalid
  saved values fall back to 1. Selecting None does not touch an audio device.
- The original oscillator, envelope, pitch, noise, filter and convolution code
  is rendered with OfflineAudioContext at build time. The original output chain
  is retained: master 0.48, 3800 Hz low-pass/Q 0.32, dry 0.88 and stereo 1.6-second
  convolver/wet 0.34, scheduled at 10 ms. The fixed random seed selects one
  reproducible realization of the original noise/reverb. This is not an
  approximate native reimplementation of the waveforms.
- `assets/completion-sounds/manifest.json` records original source and generator
  hashes, pinned baseline, browser, seed, encoding, names, durations and asset
  hashes. Two complete regenerations produced byte-identical WAVs. The fourteen
  stereo 48 kHz PCM16 assets total 4,363,196 bytes; original MIT attribution is
  kept beside them. Runtime playback uses existing Rodio and needs no WebAudio
  renderer, external service or new runtime dependency.
- Versioned private `sounds.json` owns preset and app-sounds mute on this device.
  Unknown fields survive; malformed, unreadable and newer-format state is
  preserved. Patches merge in one process-wide device controller, so concurrent
  windows do not replace each other's preset/mute. Mute is distinct from the
  microphone, live voice mode, speech output and agent-owned voice configuration.
- Accepted live `message.complete` cues are independent of focus and native OS
  notification preferences. Replayed/hydrated frames do not play. The root's
  connection/profile/runtime/authority/generation/sequence acceptance runs first.
- Mute and None are checked before claiming the one-second shared cue. Native
  and validated SDK completions share principal/profile/`sound:<runtime>` keys.
  Playback is a separate step that reads the current mute and chosen preset,
  rather than a captured stale selection. The bounded worker opens an output
  device only when it actually plays, sleeps without idle polling, and reports
  queue/decoder/output failures truthfully.
- A validated SDK sound claim queues the same native cue and returns `false` to
  its JS caller, preventing a second WebAudio playback. It uses the native
  current preset and mute even if the child continuation observes stale state.
  SDK `speak:*` ownership remains a separate ten-minute claim and does not mute
  voice or play a completion cue.
- Explicit Preview bypasses app-sounds mute and ownership arbitration. None
  remains silent. Automatic OS notification sound uses the same device mute.
  Fixture previews are disabled and never open speakers.

## Validation and limits

All six focused sound contracts passed with the final SDK central-playback
adjustment. Its contract checks SDK-first/native-first single playback, current
preset and mute after queueing, plus independent speech TTL.
All fourteen assets decode through actual Rodio as finite, non-silent, distinct
stereo data with bounded duration and matching provenance hashes. Other contracts
use a fake player: mute/None before claim, late mute/current preset, preview,
shared device patches, one-second expiry, private preferences and queue saturation.

The fourth frozen checkpoint passed all 374 Rust tests, with the explicit
virtual-device case ignored in the default run. Its separate strictly isolated
Pulse/ALSA acceptance passed, including the new CompletionSounds actor selecting
Preset 14 and Preview → production lazy Rodio output → private null-sink monitor.
Measured monitor peak was 261 PCM16. This proves the actor/output path beyond
asset decoding and the fake-player policy tests. No physical speaker, microphone
or provider was used by these checks or asset generation.

The final ten-case native feedback pass used immutable binary SHA256
`056c315e07a355a98414199b844aca6567ba7ed2ff0c4cbc62662f213af68ff8`.
Its sound checks verified fifteen preset options (fourteen originals and None),
selection of None closing the picker and updating the field, the actual mute
switch changing, fixture Preview remaining disabled, and both real background
and plugin notification categories being interactive. Light settings and the
dark preset menu were visually inspected; evidence is retained in
`/tmp/hermes-feedback-fourth-final`. This binary adds only the background
group's minimum-height/shrink correction after the preceding 374-test and
private-device proof; sound actors and assets are unchanged.

Native and SDK speech segment keys are not yet joined to the existing native
TTS pipeline; ten-minute SDK speech ownership alone does not prove cross-path
spoken-reply deduplication. Cross-process device preference reconciliation outside
the owned plugin mirror still requires acceptance. macOS/Windows output devices
and system behavior require checks on those hosts. Haptic hardware, wake/thinking
cue banks, HUD and Quick Entry remain separate work.
