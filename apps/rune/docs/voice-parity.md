# Voice behavior and native migration boundary

Audit date: 2026-10-29. The retained Hermes Desktop source is the behavior
reference. The Rune dependency inspected here is the exact revision pinned in
`apps/rune/Cargo.toml`: `bfb8facae2e4665e5e6d17afe85561884a7fd402`.
This inventory describes source contracts and remaining work; a UI capture or
headless fake microphone does not establish working native audio.

The current correction transfers the missing waveform primary button and its
separate engine chevron, alongside the existing dictation fan. Its backend
integration is limited to same-owner, read-only voice status. Engine writes,
microphone capture, transcription, playback, wake listening and conversation
execution remain unavailable. The menu must display resolved backend state,
not invent a successful local engine choice or an active voice session.

## Three distinct user actions

| Action | Result | Retained source |
| --- | --- | --- |
| Voice dictation, the inline microphone | Record one utterance; stopping transcribes it and inserts text into the draft. It never submits a prompt automatically. | `apps/desktop/src/app/chat/composer/hooks/use-voice-recorder.ts`, `voice-fan.tsx` |
| Speech-to-text + Hermes voice, the waveform primary | A hands-free loop: microphone/VAD, STT, a real Hermes turn, spoken reply, then listening again. The turn carries `voice_turn: true`; the backend resolves `auxiliary.voice_chat`, whose auto setting uses the session model. | `hooks/use-composer-voice.ts`, `hooks/use-voice-conversation.ts`; `apps/desktop/src/app/session/hooks/use-prompt-actions/submit.ts` |
| GPT-Live, the alternative conversation engine | A separate full-duplex WebRTC voice transport delegates real tasks to Hermes and speaks the results. Hermes retains model choice, tools, memory and approvals. It is not another STT-provider label. | `apps/desktop/src/lib/voice-live.ts`; `hooks/use-voice-live-conversation.ts` |

Paths beginning `hooks/` in this document are relative to
`apps/desktop/src/app/chat/composer/`.

## Composer placement and states

`apps/desktop/src/app/chat/composer/controls.tsx` computes
`showVoicePrimary = !busy && !hasComposerPayload`:

- Empty, idle composer: show the waveform primary with its separate engine
  chevron. The microphone remains the dictation action beside it.
- Text or attachments: show Send instead of the waveform primary.
- Busy conversation with empty input: show Stop. Typed input has the real
  send/steer/queue/card-answer semantics of the conversation pipeline.
- Active voice conversation: replace the ordinary controls with
  `ConversationPill`, rather than merely highlighting the idle waveform.

`start-voice-button.tsx` gives the waveform one unambiguous start action. The
chevron opens the engine menu and does not start recording. Its tooltip names
the resolved engine when known. HUD/narrow layouts use `voice-menu.tsx` to fold
the same voice controls and engine rows; `minimal` retains only the primary
action. Those responsive layouts remain later native pane work.

The active pill contains a wake ear paused for voice ownership, a mute/unmute
control, a Stop-listening control only while listening and unmuted, and End.
It exposes input level and an accessible status. Chained conversation states
are `idle`, `listening`, `transcribing`, `thinking` and `speaking`. GPT-Live
uses listening/speaking/thinking; it has no separate transcribing phase because
the remote voice model owns speech recognition. A disabled native start button
must not enter any of these states.

## Dictation and the fan

The microphone reports dictation only: `idle` shows the mic, `recording` shows
a stop square, and `transcribing` shows a spinner and refuses another start.
`use-voice-recorder.ts` tracks elapsed time, level and partial transcript, caps
the configured recording limit to 1–600 seconds, and returns to idle after
success, empty speech or failure. Recording failure and transcription failure
are distinct recoverable outcomes. Successful transcription inserts into the
draft and focuses the editor; it does not submit or clear that draft.

`voice-fan.tsx` places two independent controls above the mic:

- Speaker: automatic reading of replies. Its enabled state must not light the
  dictation microphone.
- Ear: wake-word listening, with its own listening/pending/refusal state and
  configured phrase. The fallback display phrase is "hey hermes".

`components/ui/fan-menu.tsx` owns hover/focus expansion, delayed pointer leave
and Escape dismissal. Native visual reveal can be transferred before audio,
but a cosmetic toggle is not evidence of speaking or listening.

## Engine choice is backend configuration

The authority is `voice.voice_chat_mode` in the selected backend profile, with
wire values `chained` and `gpt-live`. It is not session metadata, the model used
by archived history, a draft launch choice, or an application-global preference.

`apps/desktop/src/lib/voice-live.ts::fetchVoiceLiveStatus` reads
`GET /api/audio/voice-live/status` with the selected connection/profile. A
successful payload carries `ok`, `mode`, `available`, `reason`, `model` and
`voice`. The native read must preserve the selected owner's authentication and
private HTTP credential path, and reject stale results after owner changes.
The present native projection retains mode and availability only; detailed
backend reason/model/voice readouts remain to be transferred with voice settings.
Do not infer availability from a model/provider name or from local microphone
presence: status describes whether the gateway can start the live engine.

`voice-engine-rows.tsx` hides the engine rows before status resolves or when the
backend predates the endpoint. It checks the resolved mode, offers chained,
and disables GPT-Live when `available` is false, displaying the reason. The
legacy fallback mode is chained. Starting with an unavailable configured
GPT-Live engine falls back to chained with a visible notice in the retained
client; the native read-only slice does not start either engine.

The retained `apps/desktop/src/store/voice-live.ts::setVoiceChatMode` sends
profile-scoped `config.set {key: "voice.voice_chat_mode", value: MODE}`, then
re-reads status. Scope and write-epoch fencing prevent old reads from undoing
a newer choice. A successful write affects the next voice conversation only;
an active conversation keeps the engine latched at its start.

That mutation is **disabled in the present Rune slice**. Showing status does
not authorize the UI to check a different row optimistically or save a fake
draft override. Actual configuration writes, failure recovery and readback are
a later implementation step.

## Audio transport and ownership contracts

| Leg | Existing transport and boundary |
| --- | --- |
| Voice configuration | Owner-scoped `GET /api/audio/voice-config` describes STT/TTS and streaming support. `apps/desktop/src/lib/voice-client-direct.ts` caches by connection/profile and can call supported providers directly. Provider rejection surfaces; it is not silently retried through relay. Native can begin with the existing relay contract without exposing provider credentials. |
| Recorded STT relay | Owner-scoped `POST /api/audio/transcribe` with `{data_url, mime_type}` returns `transcript`. `apps/desktop/src/api/system.ts::transcribeAudio` uses a clip-aware deadline. `use-prompt-actions/index.ts::transcribeVoiceAudio` respects `stt.enabled !== false`. |
| STT warm-up and release | `POST /api/audio/stt-lease` with `{active, lease}`. `lib/stt-lease.ts` serializes/coalesces by lease and resolved owner. Every new acquire can re-warm an evicted model; an old client timestamp is not readiness. Warm-up starts when the mic opens and settles before audio is submitted for decoding. |
| Streaming dictation | `lib/voice-stream.ts` opens owner-scoped `/api/audio/transcribe-stream` using the canonical authenticated sibling WebSocket. Send `{sample_rate: N}`, then binary mono signed 16-bit little-endian PCM, then `{eos: true}`. Receive partial text, final transcript, or error. The client retains recorded audio for blob fallback if streaming is absent, fails, or finishes empty. |
| Speech output | `api/system.ts::speakText` posts `{text}` to owner-scoped `/api/audio/speak`. `lib/voice-playback.ts` also supports `/api/audio/speak-stream` with the speaking session's connection/profile. Native output must preserve ordering, cancellation and interruption. |
| TTS lifecycle | `POST /api/audio/tts-lease` with `{active, lease}`. `lib/tts-lease.ts` serializes toggles; warm-up is best effort. Conversation leases are per renderer/window so ending one surface does not release another surface's engine. |
| GPT-Live session | `lib/voice-live.ts::VoiceLiveSession.start` captures the local mic, creates a WebRTC offer plus `oai-events` data channel, and posts `{history, sdp}` to owner-scoped `/api/audio/voice-live/session`. The gateway returns a session identity and SDP answer. The provider key remains on the gateway. |
| GPT-Live delegation | `session.delegation.created` triggers an ordinary durable Hermes submission, carrying `surface: "voice-live"` and model-bound `voice_context`. The persisted user text remains what was spoken. Results flow back through `session.commentary.append`, progress through `session.thinking.append`; voice context is not a rendered or persisted user transcript replacement. |

A dictation's resolved owner is captured once at mic-open and reused for
warm-up, transcription and release. A native port must additionally retain the
intended draft target and reject late insertion after it is no longer valid.
An owner switch must never redirect old audio, a leased release or a transcript
to the newly selected profile. Unmount/cancel invalidates late completions.
Live-session start/end epochs close a late-created session instead of leaking a
second billed conversation. Existing routing contracts include
`lib/voice-live.routing.test.ts`, `lib/voice-playback.routing.test.ts` and
`hooks/use-voice-live-conversation.owner.test.tsx`.

Desktop automatic reading is client-owned:
`store/voice-prefs.ts` persists `hermes.desktop.autoSpeakReplies`. Backend
`voice.auto_tts` is only a one-time legacy seed; refreshing it must not override
a deliberate Desktop choice or mutate messaging-gateway behavior.

Wake listening remains backend-owned. `store/wake-word.ts` uses `wake.status`,
`wake.start {persist: true, surface: "gui", client_capture: true}` and
`wake.stop {persist: true}` for deliberate toggles. A client-capture response
requires real local PCM feeding and truthful capture failure. Before voice chat
opens the mic, `use-composer-voice.ts` awaits `wake.pause`; after ending it,
`resumeWakeAfterVoice` releases capture, resumes and reconciles against status.
Passive reconciliation never changes persisted enablement.

## What the pinned Rune framework actually supplies

Rune already has native audio APIs; saying it has no audio at all would be
incorrect. At the pinned revision:

- [`crates/rune/src/recorder/mod.rs`](https://github.com/anomalyco/rune/blob/bfb8facae2e4665e5e6d17afe85561884a7fd402/crates/rune/src/recorder/mod.rs)
  exposes `Recorder`, permission/state, metering, elapsed time and WAV/AAC file
  output. Its macOS backend uses AVAudioRecorder on its own thread. Bundled
  macOS apps require `NSMicrophoneUsageDescription` and the appropriate signing
  entitlements; source availability is not package acceptance.
- [`crates/rune/src/video/mod.rs`](https://github.com/anomalyco/rune/blob/bfb8facae2e4665e5e6d17afe85561884a7fd402/crates/rune/src/video/mod.rs)
  exposes `Player::audio` with native AVPlayer playback on macOS.
- Non-macOS desktop builds select `recorder/unsupported.rs` and
  `video/unsupported.rs`. Linux and Windows need real capture **and playback**
  integrations; a working Linux Rune window does not supply them automatically.
- Headless `FakeMicrophone` follows virtual time and writes synthetic WAV.
  Headless playback follows a silent virtual clock. They are useful test seams,
  never proof of a device, microphone permission, audible output or WebRTC.
- A file recorder/player is not the PCM callback and full-duplex WebRTC/media
  integration required for streaming STT, wake capture and GPT-Live. Those
  missing legs require explicit native implementation and acceptance.

No actual microphone, transcription, audible playback, wake listening or live
voice session is certified by this document or the current UI correction.

## Functional continuation slices

1. **Dictation first.** Add real native capture per supported platform, scoped
   permission/error handling, bounded recording, STT relay and readiness, and
   insertion into the original draft without sending. Verify success, silence,
   denial, device loss, cancellation, navigation and a late owner-switched reply.
   This can work before the durable prompt submission slice.
2. **Speech output and reply reading.** Add real playback and owner-scoped TTS,
   local preference persistence, spoken-turn tracking, ordered audio and cancel.
   Verify that two profiles use their own voices and a stale reply is not read.
3. **Chained conversation.** Build on real durable send/stream/stop, then add
   VAD/silence, voice-turn launch policy, mute/end, stop phrases, barge-in,
   wake/mic coordination and the truthful active pill. Do not reuse a fixture
   prompt handler as the conversation engine.
4. **Engine configuration.** Enable the real profile-scoped mode write with
   status readback, stale-read fencing and failure recovery. Preserve the
   active engine until the next conversation and reject unavailable starts.
5. **GPT-Live and continuous wake.** Add actual native WebRTC and PCM capture,
   secure owner-routed session/delegation transport, interruption, captions,
   teardown and mic ownership. Exercise disconnects, rapid start/end and owner
   changes without leaked sessions, microphones or engine leases.

Each stage needs real device acceptance on the platform it claims to support.
Synthetic scenes remain suitable for state and geometry checks, not a shortcut
around those audio and connection contracts.

## Verification of the read-only correction

39 Rust tests, 10 shell scenarios and 17 composer/voice scenarios passed. The
new contracts exercise authenticated owner-scoped voice status, engine versus
availability, stale reply fencing and synthetic-scene isolation from live
routes. The fixture isolation regression was reproduced red before its fix.
Native Linux X11 captures verify the actual engine menu, Escape, reopen and
outside-click dismissal with the persistent instant overlay layer;
both appearances and short-window bounds are covered with synthetic data.
A read-only Chikibryak probe returned chained selected and GPT-Live available.
No microphone, transcription, TTS, live session or settings write was invoked.
