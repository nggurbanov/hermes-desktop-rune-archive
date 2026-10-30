# Functional parity ledger

Integration checkpoint, 2026-10-30. Baseline: retained Hermes Desktop at the
repository's pinned upstream revision in `desktop-baseline.json`. This document
records current implementation; older screenshots/inventories describe the
complete target and earlier checkpoints. **Full parity is not achieved.**

| Area | Implemented now | Still required before full acceptance |
|---|---|---|
| Standalone boundary | Native client attaches existing local/URL/SSH agents; client state and packaged GUI SDK carry no Python core | Fresh Rune-only distribution, OS packaging/signing/update workflows, macOS/Windows local attach |
| Conversation transport | Durable prepared input, receipts/unknown outcomes, canonical/legacy attach, streaming/replay/stop, queue/steer, confirmations, approval/clarify, earlier-history merge, frozen multimodal edit/retry/restore/branch and scoped slash completion/directives; authoritative per-field policy stamps, live/snapshot/receipt adoption and retained deferred recovery | Remaining provider sidecars and slash directives, reactions/search/minimap and rich media players; see native-chat-parity.md |
| Composer | Updated 21:12 T3 reference: attachment at left, compact model+reasoning at right, one Reasoning/Service Tier/Permissions popover, provider search/favorites, keyboard selection and native file staging; Hermes voice controls retained; independent profile approval and confirmed conversation bypass controls; actual per-tab editor controllers, original-input file chooser/text callbacks and late policy acknowledgements | Remaining transcript/composer commands and platform accessibility acceptance |
| Subagents | Scoped live roster and separate native Rune spectator windows with lazy attachment, shared rich transcript/media, copy/download/read-aloud actions, bounded latest-only projection, reuse/focus/theme, reconnect and detach-only close | macOS/Windows window-manager acceptance; see subagent-windows-parity.md |
| Hermes shell | Original navigation order, Sessions/Bots, canonical Bot Chat isolation, conversation tabs, independent simple/advanced layouts, both OpenCode appearances | Full simultaneous session tiles, remaining fleet/legacy-default/restore behavior and narrow-window/accessibility acceptance |
| Pane tree | Native split/group model, docking, stacked tabs, divider resize, collapsed rails, persistent visibility, original Default/Basic/Focus/Terminal deck/Quad presets and named user layouts | Plugin pane geometry, simultaneous tile ownership and complete external/multiple-window acceptance |
| Sidebar | Real independent recents/messaging/cron sources, grouping, durable pin/read/archive/rename/delete, filters and paging | Whole fleet status/order/color/startup/window interactions and cross-source activity rollups |
| Bots | Profile CRUD, SOUL, config/model guard, assets/appearance generation/upload, duplication, setup/readiness, archive import/export and desktop overlay seam | Complete rail integration; all imported theme definitions and original skins; new-bot acceptance on newer external backend |
| Settings | Schema autosave/defaults, env/secrets, providers/OAuth, main/aux/MoA, memory/vault, plugins, billing, web access, local models, delegation, voice settings; real device typography/density/bubble/disclosure and editable shortcut capture | Custom skins/arbitrary system fonts/localization, remaining host/client settings and platform acceptance; see settings-parity.md and client-settings-parity.md |
| Capabilities | Real skills/hub, toolsets/config, plugin lifecycle/schema, MCP/OAuth/connectors/policy, consent and logs | Cross-platform/browser callback acceptance and every connector/provider-specific workflow; see capabilities-parity.md |
| Messaging | Sparse credential editing/clear/revert, runtime-state filters, platform enable/test, pairing, Telegram setup/QR/poll/expiry/retry/apply/cancel, restart receipts/followthrough, webhook CRUD/secret | Specialized platform workflows and acceptance against independently installed external versions |
| Artifacts | Cross-profile extraction with per-session retry, independent image-grid/file-table paging, search/filter, authenticated image/text preview, shared native image viewer with original-byte save and zoom/pan, bounded asynchronous content-sniffed raster/SVG decoding, shared native audio preview with seeking/rate/original save, streaming downloads, source-chat navigation and explicit native external-open | Simultaneous inline audio surfaces, PDF/video/HTML presentation and remaining external-host/platform media acceptance |
| Scheduled jobs | CRUD, models/delivery, pause/resume/run/delete, blueprints, runs and scheduler-owned read-only verdict | Remaining specialized field/widget acceptance on external versions |
| Command Center | Sessions/export, real health/readiness/logs, usage charts/tables, maintenance receipts/curator/memory/debug share, external update/restart recovery; all four native sections render with isolated fixtures | Remaining host-owned control integration and external lifecycle acceptance |
| Files | Local vs remote routing, tree/ignore/search/refresh, text edit/conflict-safe save/copy, local Rename/Trash, shared native image fit/zoom/pan/original save, bounded background thumbnail/preview decoding, shared native audio preview and exact original save, downloads/quote, scoped remote ignore reads and local/remote directory picker | Inline/remount audio, video/PDF/HTML preview, OS watcher and advanced preview/selection scenarios |
| Review | Native Git/controller and scoped remote API, uncommitted/branch/last-turn comparisons, stage/unstage/confirmed discard, commit/push/PR, branches/worktrees and captured turn baselines; real temp-repository tests and native populated/scrollable pane | Real remote-host and macOS/Windows acceptance, remaining narrow-pane interaction coverage |
| Terminal | Native ANSI/PTY, local/SSH and durable remote transport, tabs/input/IME/resize/clipboard/selection/persistence, owner-bound process mirrors and shared event-driven background process lifecycle; controlled real PTY/remote replay tests passed | Actual remote-host acceptance, Windows shell preference/ConPTY and all native OS interaction checks; see native-terminal-parity.md |
| Voice | Actual CPAL/Rodio, dictation/chained/wake/autoread, scoped cancellation, native Linux WebRTC/Opus/AEC3, incremental GPT-Live delegation | Actual provider integration and macOS/Windows device/permission acceptance; synthetic AEC is not a hardware measurement |
| GUI SDK | Original React/runtime/kit, retained canonical HermesGateway, authenticated brokers, events/storage/multipart/socket/native OS dialogs, verified cross-source leases and parent routing; bundled Bots groups/Petdex native acceptance; actual native SDK notification body/button callbacks, source/window leases and native focus | Remaining pane geometry/data engines/preview contributions, fleet freshness and complete parent-window/platform acceptance |
| Connections | Typed saved Local/Remote/SSH, private keyring references, transactional metadata, strict SSH forwarding, read-only readiness, retained source lease pool and verified cross-source GUI routing | Cloud/OAuth login, complete fleet presentation and legacy/default restoration and all native OS acceptance |
| Native windows | Complete Viewer factory, dynamic independent native hosts, claimed WindowId/root editors, captured source/profile/session/Bot Chat targets, local Close and global Quit commands; see native-full-windows-parity.md | Complete legacy/fleet startup, topology restore, complete source/feature/platform acceptance, dirty global Quit and final owned-service shutdown |
| Startup/default routes | Device-wide logical source/profile defaults, primary/last-used/confirmed-profile policy, native context actions and single-source startup controls, exact-profile activation and owner/input-fenced deferred navigation; see native-startup-route-parity.md | Complete persisted legacy/global/per-profile remote override recovery, project/tile scope and full fleet presentation; final native/platform acceptance |
| Client state | Locked per-field device/layout/display/sidebar/profile preference patches with explicit Reset/Clear intents and failed-write retry, principal-scoped v4 per-editor multimodal drafts/favorites with locked per-entry CAS/monotonic revisions/tombstones, migrated legacy input and durable owner-slot identities, named layouts/profile overlays, workspace/statusbar/timers/resource reads and custom shortcuts | Live multiple-window observation, full geometry/placement/restore policy, skins/arbitrary fonts/global hotkeys and remaining fleet/client controls |
| Notifications and feedback | Real Linux D-Bus delivery, device preferences, seven live source kinds including background processes and original SDK plugins, reconnect/replay/focus guards, bounded broker, owner-bound actions, native/SDK completion cue ownership and all fourteen original sounds plus None/mute/preview | Native/SDK speech-segment ownership, HUD/haptics, pure-occlusion visibility and macOS/Windows adapters; see native-notifications-parity.md, native-plugin-notifications.md and native-completion-sounds-parity.md |
| Background processes | Per-session authenticated process list/kill, running-to-exited completion, linger/output/confirmed stop/dismiss/rewind and one-shot read-only runtime recovery; visible-running safety polling and scope retirement | Full concurrent tile/peer integration and external-host/platform acceptance; see native-background-processes-parity.md |
| Audio-file playback | Actual native WAV/MP3/Vorbis/FLAC/AAC and Ogg Opus playback, paused metadata, seek/volume/mute/pitch-preserving 0.25–4x, authenticated agent files and credential-free external HTTP, bounded range/sequential transport, cancellation generations and exact original saving; pinned decoder metadata/allocation patches | Inline concurrent players/remount positions, multichannel/chained/container Opus, music-quality and native OS acceptance; video/PDF/HTML remain separate; see native-audio-player-parity.md |

## Evidence at this checkpoint

- The eighth frozen integration passed **515 Rust tests**, zero failed, five
  ignored. The private storage/ownership child helpers remain exercised by their
  passing default parents; physical audio is not part of this routing check.
  Formatting and locked offline build passed. Immutable binary SHA-256:
  `1407f2ff28abfa14e3272235a36ede4b972beb88d83d49be21d025a2da39365a`.
- That exact binary also passed **97 renderer/script-input scenarios**: editors
  5, T3 composer 19, client settings 15, shell 10, feature pages 24, rich chat 13
  and full Viewer audio navigation 11. Evidence metadata, exact commands, harness
  hashes and logs are in `/tmp/hermes-goal-eight-regressions97/result.json`;
  rich-chat artifacts are in `/tmp/hermes-chat-acceptance-PMV5H9`. These checks
  are separate from ordinary window-manager and clipboard acceptance.
- Device preferences and registry contracts exercise locked writes, unknown/future
  preservation, retained retry, metadata-only observation, primary/last-used/profile
  separation and cancelled route tickets. Application contracts prove outgoing
  history/input retention on unavailable preferences, delayed newer navigation,
  actual same-editor text changes, dirty files, profile-pinned source recovery and
  explicit same-owner actions that cancel pending profile-window navigation.
- That exact eighth binary passed **16 actual native startup/default scenarios**
  with two strict private gateways, real X11/WM pointer/keyboard input, 28 PNGs
  and normal Quit. They cover precedence, ordinary/profile-pinned window intents,
  context actions, confirmed Bot/old-tab profile memory, newer typing and explicit
  same-owner cancellation, real registry lock errors, missing profiles and
  invalid/future preference preservation/repair in both themes. No live agent,
  provider call, agent mutation or physical audio is part of this evidence.
  `native-startup-route-acceptance.md` records exact artifacts and predecessor
  RED evidence; these cases do not establish full fleet/project/platform parity.
- The same eighth binary passed the complete actual native peer-window flow:
  five distinct XIDs in one PID, four coexisting windows, independent persisted
  inputs/streams, scoped Stop, settled Settings, captured source/profile, session
  reuse, pinned Bot/New Session, interactive survival after primary close and
  local Close/global Quit. The full clipboard diagnostic passed. A separate
  two-window/three-round regression passed all six exact external GTK copies and
  OS/runtime pointer checks with normal clicks and no wiggle. The one-file winit
  X11 cursor-cache patch fixes the seventh checkpoint failure; its original
  archive, 217 upstream files, patch/addition hashes and Apache-2.0 license are
  verified offline and in Git's index. See native-peer-window-acceptance.md.
- The rich native subagent miniwindow regression also passed on that same binary:
  lazy read-only attach, real Copy, mislabeled SVG/image and tool output,
  reuse/focus/theme and viewer-only detach. Its private evidence is
  `/tmp/hermes-rune-goal-eight-subagent-native`; no microphone, physical output
  or provider was exercised. Full fleet/project and native platform/distribution
  gaps are mapped in `native-fleet-next-slices.md` and
  `native-platform-next-slices.md`.
- Durable evidence archive:
  `/home/ubuntu/Artifacts/hermes-rune-eighth-checkpoint-2026-10-30.zip`.
  Its manifest records the source commit, immutable binary, evidence file hashes,
  exact final test logs and native screenshots, with separate pre-fix failures.

### Seventh checkpoint

- The seventh frozen integration passed **481 Rust tests**, zero failed, five
  ignored. Four private storage/ownership child helpers are exercised by their
  passing default parents; the explicitly run private audio proof remains at
  the fifth checkpoint. Formatting and build passed. Immutable binary SHA-256:
  `3611c41cb30d625e95adfce43ae7e6032de6ed54b4f4f65282790b30fd8d095d`.
- That exact binary passed the same **97 renderer/input scenarios** across
  editors/composer/client settings/shell/pages/rich chat/full Viewer audio
  navigation. A separate actual Linux window-manager flow verified five distinct
  native XIDs in one PID (four coexist), independent typed/persisted owner slots,
  settled Settings, two concurrent sends/streams and owned Stop, source/profile
  capture, session target/reuse, profile-pinned Bot Chat/New Session, a surviving
  interactive peer after primary close, local Close and global Quit. See
  native-peer-window-acceptance.md for exact evidence and limits.
- The complete-composer keyboard clipboard diagnostic returned empty text and
  was **unverified at that checkpoint**. Its failure is retained explicitly, despite a separate
  minimal Rune/Kit copy proof passing. It is not counted as accepted behavior.
  The existing rich subagent miniwindow separately passed actual Copy, SVG/image,
  tool disclosure, reuse/focus/theme and viewer-only detach on the final binary.
- The recorded pinned Rune extension passed five public Runtime contracts,
  80 actual native create/close/reuse cycles and an actual no-Vulkan surface
  initialization failure. These use a separate example binary; they do not
  substitute for the complete Hermes application. All 902 staged original,
  modified and added source blobs match the manifest (888 original inputs,
  nine declared modifications, fourteen additions). Git preserves exact source
  and notice bytes; upstream whitespace remains unchanged.
- OS claims, durable journal CAS/payload fencing, exclusive window identities,
  stable primary-owned legacy migration, retained Resolver lifetime, local
  registry observation/transactions, stale connection form revisions and forced
  close-time preference flushes have actual file/process/socket contracts.
  Three duplicated-descriptor contracts first failed, then passed after all
  logical file-lock guards gained explicit unlock. The final full suite includes
  that correction; no delay or serialization masks the former parallel failure.
- The twelve actual native window screenshots and manifest are in
  `/home/ubuntu/Artifacts/hermes-rune-native-windows-2026-10-30.zip`. They use only
  synthetic private peers. Full window/default/restore behavior, concurrent
  tiles, complete feature integration and all platform/distribution gates remain
  active requirements. The next routing map is native-startup-route-parity.md.

### Sixth checkpoint

- The sixth frozen integration passed **439 Rust tests**, zero failed, three
  ignored. Two ignored storage helpers are launched by passing default parents
  in actual separate processes; the explicitly run private audio proof remains
  recorded below. Formatting and build passed. Immutable binary SHA-256:
  `1ccda04a1714fb312aeb4fa4f404e6f7c714bbe609517368f22248a2083896dc`.
- That exact binary passed **97 native renderer/input scenarios**: five editor,
  nineteen composer, fifteen client settings, ten shell, twenty-four feature
  pages, thirteen rich-chat and eleven full Viewer audio-navigation cases.
  The audio cases use the ordinary Files/Artifacts/transcript routes, including
  cross-profile file ownership, return selection, replacement and connection
  retirement. Neither these offscreen cases nor the private metadata proof
  establish macOS/Windows or window-manager acceptance.
- Shared preference contracts verify two processes waiting on the real OS lock,
  stale unrelated edits, explicit unchanged Reset/Clear/selection, nested binding
  removal, unknown/future-schema preservation and failed-save repair/retry.
  This is safe persistence, not live peer-window synchronization. Connection
  registry transactions, admission claims and window records were later added
  at the seventh checkpoint; complete restore/placement behavior still remains.
- The Linux CI workflow now declares the audio/WebKit/AEC toolchain dependencies
  used by the local build and includes editor/client-settings/feature-page native
  checks. Its YAML parses; remote CI execution and other OS jobs are not claimed.

### Fifth checkpoint

- The fifth frozen integration passed **421 Rust tests**, zero failed, two ignored.
  One ignored entry is the private-device test run explicitly; the other is an
  internal storage child entry exercised by its parent in two actual processes.
  Formatting and native build passed. Immutable binary SHA-256:
  `02bca4b8963a81fa79c1b59b57dbd5216f531f7f7e76520c94d36a7f70829d77`.
- That exact binary passed **71 native scenarios**: five independent-editor,
  nineteen updated-composer, ten shell, twenty-four feature-page and thirteen
  rich-chat scenarios. The editor script verifies distinct text/attachments and
  a file chooser completing into its original input while another tab is active.
  Its initial failed harness overlapped a held pointer press with the next click;
  an explicit 20 ms hold fixed the script and the complete five-case run passed.
- A real separate Rune window on that binary passed audio metadata, paused seek,
  physical keyboard seek, rate/mute/volume, both themes, 1024 × 520 viewport bounds,
  exact original save and owner retirement. It uses the production control
  component with a private read-only loopback file peer and never presses Play.
  These captures do not establish the full Viewer Files/Artifacts/transcript
  click path or simultaneous inline audio surfaces. Three actual application
  controller contracts separately cover audio routing, inline source ownership,
  return navigation and replacing/retiring a player.
- The final sixteen audio/resource contracts passed, including real compressed
  fixtures, Ogg Opus trim/gain/seek, Sonic stereo frequency/duration, malformed
  metadata/table headers, playback ownership and original saves. Fourteen actual
  raw/range contracts passed, including native ticket renewal, a sparse 2 GiB
  file, external credential separation, representation validation and permanent
  cancellation through standard Read helpers.
- The explicit private Pulse/ALSA output proof passed after the decoder patches:
  actual playback/pause/seek/EOF, 2x with 220 Hz preserved instead of 440 Hz,
  and a seek cancelling a genuinely stalled authenticated HTTP body. The monitor
  captured 32,616 samples at 16 kHz, peak 6,975. No physical output, microphone,
  user Pulse server, live agent or provider was used.
- The unmodified Sonic sources/license, four exact Symphonia archive checksums,
  original vendored files, recorded source patches/MPL licenses and generated
  compressed fixture hashes were verified. Per-allocation/header limits plus
  per-operation source-read quotas are concrete resource controls, not a general
  decoder RSS or music-quality measurement.

### Earlier checkpoints

- The fourth integration passed **374 Rust tests**, with one explicitly ignored
  private audio-device case. Formatting and native build passed. The default test
  run does not open the user's microphone or speakers.
- The explicit ignored audio test passed independently with a private empty
  PulseAudio daemon, null sink and private ALSA configuration. It verified actual
  CPAL/Rodio voice output and actual CompletionSounds actor playback into the
  private monitor (50,380 captured samples at 16 kHz, peak 261 for preset 14).
  Native WebRTC peer loopback and synthetic AEC3 double-talk contracts remain green.
- Fourth native evidence before the final background minimum-height correction
  used immutable binary SHA-256
  `3c06534d7a645ee72f43e131d8a0c85de8eb149f77f42eccd82e36dd75056ed6`:
  10 shell, 19 updated T3 composer, 13 rich-chat, 24 feature-page and five
  notification-settings scenarios passed. The original SDK, cross-source routing
  and SDK notification callback scripts also passed in real Wry windows.
- That binary also passed actual production Artifacts acceptance with 24 queued
  images, a mislabeled SVG, both themes and detail view; the separate native rich
  subagent window verified exact scoped reads, image/file/tool presentation,
  clipboard, window reuse, focus/theme and viewer-only detach. SVG rendering now
  uses bundled fonts and denies external resources before supplying RGBA to Rune.
- A subsequent native 1024 × 520 acceptance found the background stack could push
  a long-draft composer below the workspace. The stack now has a shrinkable
  44 px minimum with its own scrolling. Formatting, rebuilding and a final repeat
  of all **374** default Rust tests passed; corrected binary SHA-256 is
  `056c315e07a355a98414199b844aca6567ba7ed2ff0c4cbc62662f213af68ff8`.
  All ten native feedback cases passed on this corrected binary, including
  the complete composer dock and last background row in a 1024 × 520 window
  with an 18-line draft; see `native-background-processes-parity.md`.
- Actual private D-Bus contracts cover daemon delivery/actions, plugin boolean
  receipts/errors/timeout, exact callback disposal and retired source/window
  fences. Three actual native SDK proofs cover original gateway calls,
  cross-source routing, and original notification body/button/dismissal handlers.
  The generated SDK records 1,013 source inputs and 346 dependency license records.
- The preceding third checkpoint remains separate evidence: **341** default Rust
  tests, 96 native scenarios (including ten layouts and fifteen client settings)
  and rich subagent/image acceptance, all on immutable binary
  `1184780fb70e569a31d8bed1de4c69de31d03759fe99b81fac0caffeba0bc1f6`.
  The published 104-screen archive represents that earlier checkpoint.
- A real Chikibryak read-only probe returned profiles/sessions/history and
  Settings/Capabilities/Messaging/Cron reads. No live sends/config changes,
  installation or restart formed part of these checks.

Acceptance remains open until every required behavior is implemented and
verified. The inventories in `shell-parity.md`, `screenshot-parity.md`,
`lower-left-parity.md` and `voice-parity.md` preserve the complete target;
none of their earlier 'Pending' labels can be treated as evidence of current
implementation without checking this ledger and the current code.
