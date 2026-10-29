# Functional parity ledger

Integration checkpoint, 2026-10-29. Baseline: retained Hermes Desktop at the
repository's pinned upstream revision in `desktop-baseline.json`. This document
records current implementation; older screenshots/inventories describe the
complete target and earlier checkpoints. **Full parity is not achieved.**

| Area | Implemented now | Still required before full acceptance |
|---|---|---|
| Standalone boundary | Native client attaches existing local/URL/SSH agents; client state and packaged GUI SDK carry no Python core | Fresh Rune-only distribution, OS packaging/signing/update workflows, macOS/Windows local attach |
| Conversation transport | Durable prepared input, receipts/unknown outcomes, canonical/legacy attach, streaming/replay/stop, queue/steer, confirmations, approval/clarify, earlier-history merge, frozen multimodal edit/retry/restore/branch and scoped slash completion/directives; authoritative per-field policy stamps, live/snapshot/receipt adoption and retained deferred recovery | Remaining provider sidecars and slash directives, reactions/search/minimap and rich media players; see native-chat-parity.md |
| Composer | Updated 21:12 T3 reference: attachment at left, compact model+reasoning at right, one Reasoning/Service Tier/Permissions popover, provider search/favorites, keyboard selection and native file staging; Hermes voice controls retained; independent profile approval and confirmed conversation bypass controls | Remaining transcript/composer commands and platform accessibility acceptance |
| Subagents | Scoped live roster and separate native Rune spectator windows with lazy attachment, shared rich transcript/media, copy/download/read-aloud actions, bounded latest-only projection, reuse/focus/theme, reconnect and detach-only close | macOS/Windows window-manager acceptance; see subagent-windows-parity.md |
| Hermes shell | Original navigation order, Sessions/Bots, canonical Bot Chat isolation, conversation tabs, independent simple/advanced layouts, both OpenCode appearances | Full simultaneous session tiles, peer windows and narrow-window/accessibility acceptance |
| Pane tree | Native split/group model, docking, stacked tabs, divider resize, collapsed rails, persistent visibility, original Default/Basic/Focus/Terminal deck/Quad presets and named user layouts | Plugin pane geometry, simultaneous tile ownership and complete external/multiple-window acceptance |
| Sidebar | Real independent recents/messaging/cron sources, grouping, durable pin/read/archive/rename/delete, filters and paging | Whole fleet status/order/color/startup/window interactions and cross-source activity rollups |
| Bots | Profile CRUD, SOUL, config/model guard, assets/appearance generation/upload, duplication, setup/readiness, archive import/export and desktop overlay seam | Complete rail integration; all imported theme definitions and original skins; new-bot acceptance on newer external backend |
| Settings | Schema autosave/defaults, env/secrets, providers/OAuth, main/aux/MoA, memory/vault, plugins, billing, web access, local models, delegation, voice settings; real device typography/density/bubble/disclosure and editable shortcut capture | Custom skins/arbitrary system fonts/localization, remaining host/client settings and platform acceptance; see settings-parity.md and client-settings-parity.md |
| Capabilities | Real skills/hub, toolsets/config, plugin lifecycle/schema, MCP/OAuth/connectors/policy, consent and logs | Cross-platform/browser callback acceptance and every connector/provider-specific workflow; see capabilities-parity.md |
| Messaging | Sparse credential editing/clear/revert, runtime-state filters, platform enable/test, pairing, Telegram setup/QR/poll/expiry/retry/apply/cancel, restart receipts/followthrough, webhook CRUD/secret | Specialized platform workflows and acceptance against independently installed external versions |
| Artifacts | Cross-profile extraction with per-session retry, independent image-grid/file-table paging, search/filter, authenticated image/text preview, shared native image viewer with original-byte save and zoom/pan, bounded asynchronous content-sniffed raster/SVG decoding, streaming downloads, source-chat navigation and explicit native external-open | Inline PDF/audio/video/HTML presentation and acceptance of all external-host media paths |
| Scheduled jobs | CRUD, models/delivery, pause/resume/run/delete, blueprints, runs and scheduler-owned read-only verdict | Remaining specialized field/widget acceptance on external versions |
| Command Center | Sessions/export, real health/readiness/logs, usage charts/tables, maintenance receipts/curator/memory/debug share, external update/restart recovery; all four native sections render with isolated fixtures | Remaining host-owned control integration and external lifecycle acceptance |
| Files | Local vs remote routing, tree/ignore/search/refresh, text edit/conflict-safe save/copy, local Rename/Trash, shared native image fit/zoom/pan/original save, bounded background thumbnail/preview decoding, downloads/quote, scoped remote ignore reads and local/remote directory picker | Full media/PDF/HTML preview, OS watcher and advanced preview/selection scenarios |
| Review | Native Git/controller and scoped remote API, uncommitted/branch/last-turn comparisons, stage/unstage/confirmed discard, commit/push/PR, branches/worktrees and captured turn baselines; real temp-repository tests and native populated/scrollable pane | Real remote-host and macOS/Windows acceptance, remaining narrow-pane interaction coverage |
| Terminal | Native ANSI/PTY, local/SSH and durable remote transport, tabs/input/IME/resize/clipboard/selection/persistence, owner-bound process mirrors and shared event-driven background process lifecycle; controlled real PTY/remote replay tests passed | Actual remote-host acceptance, Windows shell preference/ConPTY and all native OS interaction checks; see native-terminal-parity.md |
| Voice | Actual CPAL/Rodio, dictation/chained/wake/autoread, scoped cancellation, native Linux WebRTC/Opus/AEC3, incremental GPT-Live delegation | Actual provider integration and macOS/Windows device/permission acceptance; synthetic AEC is not a hardware measurement |
| GUI SDK | Original React/runtime/kit, retained canonical HermesGateway, authenticated brokers, events/storage/multipart/socket/native OS dialogs, verified cross-source leases and parent routing; bundled Bots groups/Petdex native acceptance; actual native SDK notification body/button callbacks, source/window leases and native focus | Remaining pane geometry/data engines/preview contributions, fleet freshness and complete parent-window/platform acceptance |
| Connections | Typed saved Local/Remote/SSH, private keyring references, transactional metadata, strict SSH forwarding, read-only readiness, retained source lease pool and verified cross-source GUI routing | Cloud/OAuth login, complete fleet presentation/default-startup restoration and all native OS acceptance |
| Client state | Native device prefs with failed-write retry, principal-scoped persisted multimodal drafts/favorites with revision-gated saves, named layouts/profile overlays, workspace/statusbar/timers/resource reads and custom shortcuts | Multiple-window reconciliation, skins/arbitrary fonts/global hotkeys and remaining fleet/client controls |
| Notifications and feedback | Real Linux D-Bus delivery, device preferences, seven live source kinds including background processes and original SDK plugins, reconnect/replay/focus guards, bounded broker, owner-bound actions, native/SDK completion cue ownership and all fourteen original sounds plus None/mute/preview | Native/SDK speech-segment ownership, HUD/haptics, pure-occlusion visibility and macOS/Windows adapters; see native-notifications-parity.md, native-plugin-notifications.md and native-completion-sounds-parity.md |
| Background processes | Per-session authenticated process list/kill, running-to-exited completion, linger/output/confirmed stop/dismiss/rewind and one-shot read-only runtime recovery; visible-running safety polling and scope retirement | Full concurrent tile/peer integration and external-host/platform acceptance; see native-background-processes-parity.md |

## Evidence at this checkpoint

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
