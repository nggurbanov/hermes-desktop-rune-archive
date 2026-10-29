# Functional parity ledger

Integration checkpoint, 2026-10-29. Baseline: retained Hermes Desktop at the
repository's pinned upstream revision in `desktop-baseline.json`. This document
records current implementation; older screenshots/inventories describe the
complete target and earlier checkpoints. **Full parity is not achieved.**

| Area | Implemented now | Still required before full acceptance |
|---|---|---|
| Standalone boundary | Native client attaches existing local/URL/SSH agents; client state and packaged GUI SDK carry no Python core | Fresh Rune-only distribution, OS packaging/signing/update workflows, macOS/Windows local attach |
| Conversation transport | Durable prepared input, receipts/unknown outcomes, canonical/legacy attach, streaming/replay/stop, queue/steer, confirmations, approval/clarify, earlier-history merge, frozen multimodal edit/retry/restore/branch and scoped slash completion/directives | Authoritative session-policy synchronization, remaining provider sidecars and slash directives, reactions/search/minimap and rich media players; see native-chat-parity.md |
| Composer | Updated 21:12 T3 reference: attachment at left, compact model+reasoning at right, one Reasoning/Service Tier/Permissions popover, provider search/favorites, keyboard selection and native file staging; Hermes voice controls retained | Remaining transcript/composer commands and platform accessibility acceptance |
| Subagents | Scoped live roster and separate native Rune spectator windows with lazy attachment, shared rich transcript/media, copy/download/read-aloud actions, bounded latest-only projection, reuse/focus/theme, reconnect and detach-only close | macOS/Windows window-manager acceptance; see subagent-windows-parity.md |
| Hermes shell | Original navigation order, Sessions/Bots, canonical Bot Chat isolation, conversation tabs, independent simple/advanced layouts, both OpenCode appearances | Full simultaneous session tiles, peer windows and narrow-window/accessibility acceptance |
| Pane tree | Native split/group model, docking, stacked tabs, divider resize, collapsed rails, persistent visibility, original Default/Basic/Focus/Terminal deck/Quad presets and named user layouts | Plugin pane geometry, simultaneous tile ownership and complete external/multiple-window acceptance |
| Sidebar | Real independent recents/messaging/cron sources, grouping, durable pin/read/archive/rename/delete, filters and paging | Whole fleet status/order/color/startup/window interactions and cross-source activity rollups |
| Bots | Profile CRUD, SOUL, config/model guard, assets/appearance generation/upload, duplication, setup/readiness, archive import/export and desktop overlay seam | Complete rail integration; all imported theme definitions and original skins; new-bot acceptance on newer external backend |
| Settings | Schema autosave/defaults, env/secrets, providers/OAuth, main/aux/MoA, memory/vault, plugins, billing, web access, local models, delegation, voice settings; real device typography/density/bubble/disclosure and editable shortcut capture | Custom skins/arbitrary system fonts/localization, remaining host/client settings and platform acceptance; see settings-parity.md and client-settings-parity.md |
| Capabilities | Real skills/hub, toolsets/config, plugin lifecycle/schema, MCP/OAuth/connectors/policy, consent and logs | Cross-platform/browser callback acceptance and every connector/provider-specific workflow; see capabilities-parity.md |
| Messaging | Sparse credential editing/clear/revert, runtime-state filters, platform enable/test, pairing, Telegram setup/QR/poll/expiry/retry/apply/cancel, restart receipts/followthrough, webhook CRUD/secret | Specialized platform workflows and acceptance against independently installed external versions |
| Artifacts | Cross-profile extraction with per-session retry, independent image-grid/file-table paging, search/filter, authenticated image/text preview, zoom, streaming downloads, source-chat navigation and explicit native external-open | Inline PDF/audio/video/HTML presentation and acceptance of all external-host media paths |
| Scheduled jobs | CRUD, models/delivery, pause/resume/run/delete, blueprints, runs and scheduler-owned read-only verdict | Remaining specialized field/widget acceptance on external versions |
| Command Center | Sessions/export, real health/readiness/logs, usage charts/tables, maintenance receipts/curator/memory/debug share, external update/restart recovery; all four native sections render with isolated fixtures | Remaining host-owned control integration and external lifecycle acceptance |
| Files | Local vs remote routing, tree/ignore/search/refresh, text edit/conflict-safe save/copy, local Rename/Trash, images/download/quote, scoped remote ignore reads and local/remote directory picker | Full media/PDF/HTML preview, OS watcher and advanced preview/selection scenarios |
| Review | Native Git/controller and scoped remote API, uncommitted/branch/last-turn comparisons, stage/unstage/confirmed discard, commit/push/PR, branches/worktrees and captured turn baselines; real temp-repository tests and native populated/scrollable pane | Real remote-host and macOS/Windows acceptance, remaining narrow-pane interaction coverage |
| Terminal | Native ANSI/PTY, local/SSH and durable remote transport, tabs/input/IME/resize/clipboard/selection/persistence, owner-bound process mirrors/polling; controlled real PTY/remote replay tests passed | Actual remote-host acceptance, Windows shell preference/ConPTY and all native OS interaction checks; see native-terminal-parity.md |
| Voice | Actual CPAL/Rodio, dictation/chained/wake/autoread, scoped cancellation, native Linux WebRTC/Opus/AEC3, incremental GPT-Live delegation | Actual provider integration and macOS/Windows device/permission acceptance; synthetic AEC is not a hardware measurement |
| GUI SDK | Original React/runtime/kit, retained canonical HermesGateway, authenticated brokers, events/storage/multipart/socket/native OS dialogs, verified cross-source leases and parent routing; bundled Bots groups/Petdex native acceptance | Remaining plugin pane geometry/data engines/preview contributions, fleet freshness and complete parent-window/platform acceptance |
| Connections | Typed saved Local/Remote/SSH, private keyring references, transactional metadata, strict SSH forwarding, read-only readiness, retained source lease pool and verified cross-source GUI routing | Cloud/OAuth login, complete fleet presentation/default-startup restoration and all native OS acceptance |
| Client state | Native device prefs with failed-write retry, principal-scoped persisted multimodal drafts/favorites with revision-gated saves, named layouts/profile overlays, workspace/statusbar/timers/resource reads and custom shortcuts | Authoritative policy sync, multiple-window reconciliation, skins/arbitrary fonts/global hotkeys/notifications and remaining fleet/client controls |

## Evidence at this checkpoint

- The second frozen integration passed **304 Rust tests**, with one explicitly ignored
  virtual-device case. Formatting and native build passed. The isolated device
  roundtrip was verified separately earlier.
- Native checks against an immutable second-checkpoint binary passed: 10 Hermes
  shell scenarios, 19 updated T3 composer scenarios, 10 layout scenarios, 15
  client-settings scenarios and 13 rich-chat scenarios. The preceding checkpoint
  also passed 24 feature-page checks. Actual native subagent acceptance proved
  rich image/file/tool presentation, copy, production registry reuse,
  focus/theme changes and viewer-only close against a controlled gateway.
- The explicit private Pulse/ALSA CPAL/Rodio roundtrip passed independently.
  Native WebRTC peer loopback and synthetic AEC3 double-talk tests passed.
- Actual Wry/original SDK smoke proved retained canonical session create/rename,
  scoped RPC, upload, namespace events, React hooks, storage and disposal with
  controlled peers. The bundled Bots group dialog rendered with two synthetic
  identities; no production bot was changed.
- A real Chikibryak read-only probe returned profiles/sessions/history and
  Settings/Capabilities/Messaging/Cron reads. No live sends/config changes,
  installation or restart formed part of those checks.

Acceptance remains open until every required behavior is implemented and
verified. The inventories in `shell-parity.md`, `screenshot-parity.md`,
`lower-left-parity.md` and `voice-parity.md` preserve the complete target;
none of their earlier 'Pending' labels can be treated as evidence of current
implementation without checking this ledger and the current code.
