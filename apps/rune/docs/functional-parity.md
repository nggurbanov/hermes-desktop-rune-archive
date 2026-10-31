# Functional parity ledger

Integration checkpoint, 2026-10-30. Baseline: retained Hermes Desktop at the
repository's pinned upstream revision in `desktop-baseline.json`. This document
records current implementation; older screenshots/inventories describe the
complete target and earlier checkpoints. **Full parity is not achieved.**

Scope decision: the managed Nous/Hermes Cloud service (portal sign-in, organization
discovery and hosted-agent selection) is excluded from this parity phase. Existing
Cloud configuration is preserved and is not automatically connected. Generic URL
connections, access headers and generic remote OAuth remain in scope.

Current testing and builds are limited to Linux. macOS and Windows remain future
acceptance targets and are not represented as verified by this phase.

| Area | Implemented now | Still required before full acceptance |
|---|---|---|
| Standalone boundary | Native client attaches existing local/URL/SSH agents; client state and packaged GUI SDK carry no Python core | Fresh Rune-only distribution, OS packaging/signing/update workflows, macOS/Windows local attach |
| Conversation transport | Durable prepared input, receipts/unknown outcomes, canonical/legacy attach, streaming/replay/stop, queue/steer, confirmations, approval/clarify, earlier-history merge, frozen multimodal edit/retry/restore/branch and scoped slash completion/directives; authoritative per-field policy stamps, live/snapshot/receipt adoption and retained deferred recovery | Remaining provider sidecars and slash directives, reactions, broader search acceptance, minimap and rich media players; see native-chat-parity.md |
| Composer | Updated 21:12 T3 reference: attachment at left, compact model+reasoning at right, one Reasoning/Service Tier/Permissions popover, provider search/favorites, keyboard selection and native file staging; Hermes voice controls retained; independent profile approval and confirmed conversation bypass controls; actual per-tab editor controllers, original-input file chooser/text callbacks and late policy acknowledgements | Remaining transcript/composer commands and platform accessibility acceptance |
| Subagents | Scoped live roster and separate native Rune spectator windows with lazy attachment, shared rich transcript/media, copy/download/read-aloud actions, bounded latest-only projection, reuse/focus/theme, reconnect and detach-only close | macOS/Windows window-manager acceptance; see subagent-windows-parity.md |
| Hermes shell | Original navigation order, Sessions/Bots, canonical Bot Chat isolation, conversation tabs, independent simple/advanced layouts, both OpenCode appearances; source-qualified fleet rail/condensed and hidden-rail pickers with correct fresh/current profile semantics | Simultaneous session tiles are not implemented; remaining fleet attention/legacy-default/persisted restore behavior and narrow-window/accessibility acceptance |
| Pane tree | Native split/group model, docking, stacked tabs, divider resize, collapsed rails, persistent visibility, original Default/Basic/Focus/Terminal deck/Quad presets and named user layouts | Plugin pane geometry, simultaneous tile ownership and complete external/multiple-window acceptance |
| Sidebar | Real independent recents/messaging/cron sources, grouping, durable pin/read/archive/rename/delete, filters and paging; source-qualified row keys/controllers, exact-owner at-rest edits and captured session windows, request-owned actor/receipt claims | Complete unlisted/hidden/persisted cross-source activity rollups, remaining project mutations and worktrees, and gesture/locale/platform acceptance; see native-fleet-parity.md |
| Bots | Profile CRUD, SOUL, config/model guard, assets/appearance generation/upload, duplication, setup/readiness, archive import/export and desktop overlay seam | Complete rail integration; all imported theme definitions and original skins; new-bot acceptance on newer external backend |
| Settings | Schema autosave/defaults, env/secrets, providers/OAuth, main/aux/MoA, memory/vault, plugins, billing, web access, local models, delegation, voice settings; real device typography/density/bubble/disclosure and editable shortcut capture | Custom skins/arbitrary system fonts/localization, remaining host/client settings and platform acceptance; see settings-parity.md and client-settings-parity.md |
| Capabilities | Real skills/hub, toolsets/config, plugin lifecycle/schema, MCP/OAuth/connectors/policy, consent and logs | Cross-platform/browser callback acceptance and every connector/provider-specific workflow; see capabilities-parity.md |
| Messaging | Sparse credential editing/clear/revert, runtime-state filters, platform enable/test, pairing, Telegram setup/QR/poll/expiry/retry/apply/cancel, restart receipts/followthrough, webhook CRUD/secret | Specialized platform workflows and acceptance against independently installed external versions |
| Artifacts | Cross-profile extraction with per-session retry, independent image-grid/file-table paging, search/filter, authenticated image/text preview, shared native image viewer with original-byte save and zoom/pan, bounded asynchronous content-sniffed raster/SVG decoding, shared native audio preview with seeking/rate/original save, streaming downloads, source-chat navigation and explicit native external-open | Broader simultaneous inline audio acceptance, PDF/video/HTML presentation and remaining external-host/platform media acceptance |
| Scheduled jobs | CRUD, models/delivery, pause/resume/run/delete, blueprints, runs and scheduler-owned read-only verdict | Remaining specialized field/widget acceptance on external versions |
| Command Center | Sessions/export, real health/readiness/logs, usage charts/tables, maintenance receipts/curator/memory/debug share, external update/restart recovery; all four native sections render with isolated fixtures | Remaining host-owned control integration and external lifecycle acceptance |
| Files | Local vs remote routing, tree/ignore/search/refresh, text edit/conflict-safe save/copy, local Rename/Trash, shared native image fit/zoom/pan/original save, bounded background thumbnail/preview decoding, shared native audio preview and exact original save, downloads/quote, scoped remote ignore reads and local/remote directory picker | Remaining directive/image-form audio and broader remount acceptance, video/PDF/HTML preview, OS watcher and advanced preview/selection scenarios |
| Review | Native Git/controller and scoped remote API, uncommitted/branch/last-turn comparisons, stage/unstage/confirmed discard, commit/push/PR, branches/worktrees and captured turn baselines; real temp-repository tests and native populated/scrollable pane | Real remote-host and macOS/Windows acceptance, remaining narrow-pane interaction coverage |
| Terminal | Native ANSI/PTY, local/SSH and durable remote transport, tabs/input/IME/resize/clipboard/selection/persistence, owner-bound process mirrors and shared event-driven background process lifecycle; controlled real PTY/remote replay tests passed | Actual remote-host acceptance, Windows shell preference/ConPTY and all native OS interaction checks; see native-terminal-parity.md |
| Voice | Actual CPAL/Rodio, dictation/chained/wake/autoread, scoped cancellation, native Linux WebRTC/Opus/AEC3, incremental GPT-Live delegation | Actual provider integration and macOS/Windows device/permission acceptance; synthetic AEC is not a hardware measurement |
| GUI SDK | Original React/runtime/kit, retained canonical HermesGateway, authenticated brokers, events/storage/multipart/socket/native OS dialogs, verified cross-source leases and parent routing; bundled Bots groups/Petdex native acceptance; actual native SDK notification body/button callbacks, source/window leases and native focus | Remaining pane geometry/data engines/preview contributions, fleet freshness and complete parent-window/platform acceptance |
| Connections | Typed saved Local/Remote/SSH, private keyring references, transactional metadata, strict SSH forwarding, read-only readiness, retained source lease pool and verified cross-source GUI routing; bounded passive fleet inventory, noninteractive Linux credential reads and shared identity cache | Further generic remote OAuth acceptance, remaining fleet/legacy/default restoration and macOS/Windows acceptance; managed Nous/Hermes Cloud is excluded |
| Native windows | Complete Viewer factory, dynamic independent native hosts, claimed WindowId/root editors, captured source/profile/session/Bot Chat targets and session titles, local Close and global Quit commands; at-rest source session windows and per-source actual editor/tab/stream retention; see native-full-windows-parity.md | Complete legacy startup, persisted per-source workspace/topology restore, complete source/feature/platform acceptance, dirty global Quit and final owned-service shutdown |
| Startup/default routes | Device-wide logical source/profile defaults, primary/last-used/confirmed-profile policy, native context actions and startup controls, exact-profile activation and owner/input-fenced deferred navigation; see native-startup-route-parity.md | Complete persisted legacy/global/per-profile remote override recovery, project/tile scope and remaining fleet attention/legacy consumers; final native/platform acceptance |
| Client state | Locked per-field device/layout/display/sidebar/profile preference patches with explicit Reset/Clear intents and failed-write retry, principal-scoped v4 per-editor multimodal drafts/favorites with locked per-entry CAS/monotonic revisions/tombstones, migrated legacy input and durable owner-slot identities, named layouts/profile overlays, exact authored rail colors/archive seam, source-owned fleet group preferences, workspace/statusbar/timers/resource reads and custom shortcuts | Full per-source tab persistence, live multiple-window observation, full geometry/placement/restore policy, skins/arbitrary fonts/global hotkeys and remaining fleet/client controls |
| Notifications and feedback | Real Linux D-Bus delivery, device preferences, seven live source kinds including background processes and original SDK plugins, reconnect/replay/focus guards, bounded broker, owner-bound actions, native/SDK completion cue ownership and all fourteen original sounds plus None/mute/preview | Native/SDK speech-segment ownership, HUD/haptics, pure-occlusion visibility and macOS/Windows adapters; see native-notifications-parity.md, native-plugin-notifications.md and native-completion-sounds-parity.md |
| Background processes | Per-session authenticated process list/kill, running-to-exited completion, linger/output/confirmed stop/dismiss/rewind and one-shot read-only runtime recovery; visible-running safety polling and scope retirement | Full concurrent tile/peer integration and external-host/platform acceptance; see native-background-processes-parity.md |
| Audio-file playback | Actual native WAV/MP3/Vorbis/FLAC/AAC and Ogg Opus playback, paused metadata, seek/volume/mute/pitch-preserving 0.25–4x, authenticated agent files and credential-free external HTTP, bounded range/sequential transport, cancellation generations and exact original saving; transcript FILE controls with independent mounted lifetimes, local Markdown audio links and assistant MEDIA audio mapped to separate inline players; pinned decoder metadata/allocation patches | Wider MEDIA streaming/lifetime and native acceptance beyond the bounded case below, remaining directive/image-form audio mapping, same-instance ticket refresh and expanded keyboard/native acceptance, multichannel/chained/container Opus, music-quality and native OS acceptance; video/PDF/HTML remain separate; see native-audio-player-parity.md |

## Everyday alpha status (2026-10-31)

A private authenticated native fixture exercised two concurrent Bot-profile
streams, background completion, distinct drafts and owner-scoped Stop on the
packaged-resource candidate. The same run selected both fixture models and
submitted the second. The foreground history race has a focused regression.
The registered ordinary-session scenario and external model/provider acceptance
remain pending; this is not full alpha acceptance.

## Recent integration evidence

The new Linux development checkpoint is bound to marker
`/tmp/hermes-rune-markdown-linux-green-20261030-n8nQJPBB.json` and manifest
`/tmp/hermes-rune-markdown-checkpoint-20261030-n8nQJPBB/manifest.json` (SHA-256
`93f6a6840fecfe2a547034688b9618cfeefbb3cbaffdd6d2c69bc06534397558`). Its
production ELF is `625c6cf51d8b959c9d89f30b5a0d3f14c914fda8020d18fb275b723850ab574b`,
test ELF `fdb8da70225574e0f589ec0a43b3bbda5ebbfaa55b911c3b1d3f0154299f89d8`,
source archive `1058f2c84e486c7f5f9c01577159b6637b1d37c0a1a61bb4a0bd3331dd612b26`
and index `85ba1bb9d0fff8491e3d442d14c9bd81dd5055c2245fe20ccce8d87f335bf760`.
All 6,493 paths and the archive were independently verified against source after
capture. The Linux host build succeeded; the required vendor unit passed after
fixing two test-only Renderer initializers, 106 vendor integration tests passed,
and vendor verification and forward/reverse patch checks passed. The 888 common
passes, 14 ignored and six private passes were reused from the previous capture
with the identical test ELF, not rerun in this build. Broad and scoped format
checks still report existing dirty differences; offline full metadata lacks
`shared_library` v0.1.9. This is not an all-checks-green or all-OS claim.

The historical failed marker
`/tmp/hermes-rune-markdown-linux-failed-gate-20261030-EY26QGqq.json` remains
unverified. It shares the production ELF hash, but its source archive, index and
manifest differ after the test fixtures and provenance were fixed; the ELF hash
alone cannot identify the new checkpoint. This ledger update is post-capture and
outside the new source archive.

At the new capture, local absolute paths and supported file URIs in Markdown audio
links map to existing owned inline players, with distinct FILE instances for
repeated paths. Hidden link aliases are excluded from search, displayed filenames
are indexed, and surrounding highlights remain; ordinary HTTP and code are
unchanged. Review regressions reproduced and fixed partial-selection copying of
hidden embedded source and lost empty-label occurrences/callbacks, without adding
empty layout by default. Evidence: `/tmp/hermes-markdown-review-red.log`,
`/tmp/hermes-markdown-review-callback-red.log`,
`/tmp/hermes-markdown-review-embed-final-format.log` and
`/tmp/hermes-markdown-review-common-final.log`. The scoped WAV test uses a
cfg(test) null output, not native sound.

The older 91f8 bounded structured-FILE WAV light/dark proof remains in
`/tmp/hermes-file-audio-accept-1793379237-367657/REPORT.md`. The recovered
`/tmp/hermes-file-lifetime-20261030-YtkT9b3Z/REPORT.md` is partial: both themes
observed absent controls, retired private PCM and return paused at zero, but the
presented-frame filter cannot prove unmount. Dark B played after fresh requests;
held A, return to A and distinct-profile isolation were unrun, light hit a pointer
timeout, and rate effect was unrun. Neither assigned gate passed and no product
failure was established. GPU ownership was explicitly released.

At this historical point, native Markdown and six representative codec cases were unproved;
their then-running checks had not reported final results. The completed post-freeze MEDIA
source slice below is not part of this frozen snapshot. Node protocol self-tests
and FFmpeg-validated fixtures are not Rune native codec proof. Image-form audio,
tables, refresh, full inline typography, codec variants and current-source native
and other-OS acceptance remain open.

**Post-freeze assistant MEDIA source:** View and search share normalized display
Markdown for assistant messages while raw history remains untouched. Each audio
MEDIA occurrence has an independent player keyed by its full owner; agent paths
use the scoped route, and decoded HTTP(S) MEDIA targets use a credential-free
external range source. Ordinary external Markdown links remain ordinary links;
non-audio MEDIA activation is unsupported or unverified. One Rust test executed
52 oracle cases (34 normalization, 11 accumulated-stream and seven review cases):
the initial RED found three whitespace/root-URL mismatches, then all 52 passed.
Private real Runtime tests with null output passed two MEDIA plus FILE controls,
independent play, prefix retention, removal/remount and search; an external range
URL retained its query without agent authorization, cookie or ticket. These are
not native PCM tests. A temporary `message.delta` → actual queue →
`Update::Changed` experiment passed, but its test and queue hook were removed;
there is no retained regression at that boundary in this completed source point.

The completed common run passed 891, failed one and ignored 14: a private
subagent socket timed out at 200 ms. Its exact isolated recheck passed, but the
whole common run was not rerun and remains failed; logs are recorded in
native-audio-player-parity.md. Quote closure, partial classification and path
changes during streaming, backfill, source and late-event proof, and native MEDIA
were open at that source point. The later bounded native streaming result below
supersedes that blanket MEDIA status only for its stated cases. Project Create
remains open; its later source and check status is recorded below.

The sealed post-rename/editor Linux candidate is recorded in
`/tmp/hermes-rune-post-rename-editor-linux-checkpoint-20261030-8pIh5qBJ.json`
with artifacts and logs in `/tmp/hermes-rune-post-rename-editor-checkpoint-20261030-8pIh5qBJ`.
ELF SHA-256 `91f809abd6bdd82f8e9951b47dce4ad396ef2774417cf9b0d9d3412357f1506b`,
test executable `1832f6c27f73c6fc38373e140e4501c0285e20f49d175e58fdb67fe0cdbe48cf`,
source archive `39be347c1efff6d733b5597f7dfcee537b3a1bd0bd9db93e7336fb91f5fc96ac`,
index `c3f66806c7f73755fecfc54dc7844d39237bbb02bf3c925df68a1da7ab4255d8`,
and manifest `0a4a6b4aab61cbd53bf304899afdcb012440bad9cf786c1e6a4ca45d585acca8`
bind 6,489 indexed paths. Common tests passed 884, failed 0, ignored 14;
six exact private tests passed. The initial long Unix-socket path caused six
fixture failures; the short-path rerun passed. Linux build and tests succeeded;
broad formatting failed on existing dirty differences, and offline full metadata
lacked an uncached cross-platform dependency. This ledger update is outside the
frozen archive and the evidence does not cover newer source.

`/tmp/hermes-91f8-native-NujZSz/REPORT.md` records actual Linux light/dark
editor Ctrl+F from the scoped editor without composer fallback, typing/paste,
visible match indices, navigation and cancellation; selection endpoints and IME
remain unproved. Overview and entered-project header Rename passed in both themes,
including captured source A/default/id/name update and refreshed label. NativeSelf,
held-response and source-fence cases remain unrun. The recorded owned processes
exited; there was no complete transient descendant census.

The earlier combined Linux development snapshot is recorded in
`/tmp/hermes-rune-combined-linux-checkpoint-20261030-1793372534.json`.
Its locked offline common suite passed 872 tests, zero failed, with 14 ignored; six exact
private SSH/OAuth contracts passed separately on its frozen test executable, each with 885 filtered.
The 6,484-source-path archive (SHA-256 `aef3a0ad4ff671f7123793ffeabc6e9aecdd88d9ce929f30d0301ba3904ac635`) was checked against the source before and after capture
and again after the build. The repository-wide formatting check failed on
existing dirty-source differences; its log is retained with the candidate.
This paragraph was added after that snapshot and is not part of its archive.
The binary SHA-256 is `dc20959bb9e79fd2201edd9603af397cb637d314282de66ce6ce275a92aae455`.

Subsequent evidence is bounded by its own snapshot and does not change that marker:

- **Post-freeze FILE audio:** actual controls and Rune `El::on_mount_change`
  implement mounted lifetime retirement. `/tmp/hermes-inline-final-retirement.log`
  records 6 passed, 0 failed, 884 filtered, including overscan, rerender and
  mount retirement and a real null-output Runtime loopback; the independent decoder
  check records 1 passed, 889 filtered in `/tmp/hermes-inline-decoder-recheck.log`.
  That Runtime proof did not exercise physical devices or native UI; this source is
  not in the dc2095 archive. The later 91f8 native FILE acceptance is described
  in native-audio-player-parity.md; remaining directive/image-form mapping, same-instance
  credential refresh and wider keyboard/native acceptance remain.
- **Post-freeze project rename:** `app/fleet/projects/rename.rs` retains captured
  scope through a separate write worker using `projects.update`. The dated
  `/tmp/hermes-project-rename-focused-20261030.log` records 3 private loopback
  passes, and `/tmp/hermes-project-rename-common-20261030.log` records 879 passed,
  0 failed, 14 ignored. These are a changing source snapshot, not a frozen binary
  or the executable behind the six dc2095 private passes. Its earlier row-button
  presentation is superseded for the tested overview and entered-project rename
  cases by the 91f8 native report above. Other project mutations and Git worktrees
  remain open.
- **Installed SDK:** the dc2095 AppImage at `/tmp/Hermes combined sdk ü 1793372534`
  has SHA-256 `40cc7348cac939d7172096e45e4b27ef60c83f38c75e0b5c04fdf909552ffdf0`.
  Both themes showed stable child PID/start time/XID, minimized restore/focus,
  state callback 1 → 2 and global Quit. Normal window-manager close and fresh
  reopen passed only in light. External XID destruction is not a normal WM-close
  defect; Alt+F4 remains unclassified. Source/profile switching and pending
  callbacks were unrun. Fifty known PIDs were audited, not a full transient census.
- **Projects preview:** `/tmp/hermes-project-native-final-20261030-1/REPORT.md`
  and its result, request and gesture records show native light/dark checkbox
  requests 3 → 2000 → 3, fourth-row appearance/disappearance, per-project
  hydration and a newly counted exact-profile resume/history. A clean restart
  preserved the preference. Home and unrelated recents, duplicate suppression,
  broad sort/filter, non-Project menu, cross-source held replies, NativeSelf and
  error/retry were unrun.
- **Search:** `/tmp/hermes-search-native-acceptance-report-20261030.md` records
  native Ctrl+F/G/Shift+G and visible marks in both themes. The later
  `/tmp/hermes-search-two-case-20261030/report.md` proves disclosure close/reopen
  and a deep grouped mark fully inside the inner 360px and outer 218px clips in
  both themes. Earlier helper failures did not establish product defects. The
  scoped editor case is subsequently covered by the 91f8 report above; broader
  live updates, owner, IME, wrap and shortcut exclusion remain unproved natively.
- **Manual Linux package lifecycle:** `/tmp/hermes-rune-lifecycle-uc8jrs/report.txt`
  and `report.json` record 12 cases plus two harness tests using
  `apps/rune/scripts/package-installed-lifecycle.mjs`. Frozen AppRun and ELF ran
  read-only loopback from a spaced Unicode path; state survived replacement and
  removal, with notices and source hashes bound to the packages. Desktop Exec's
  bare binary requires PATH or desktop integration. No installer, atomic rollback,
  signed updater feed, protocol, tray or full installed-child lifecycle is proved.
- **Concurrent tiles:** `/tmp/hermes-rune-simultaneous-tiles-contract-20261030.md`
  is a source-only proposal, not an implemented feature; its 17-row control
  inventory is partial. Two hot-hidden workspaces per zone are a lifecycle limit,
  not a two-open-tile cap. Approval, policy, search, media and restore mappings
  remain incomplete. The source-only busy-close trace in
  `/tmp/hermes-rune-busy-close-contract-20261030/report.md` shows close removes
  the view and registration without interrupting; the shared window/Secondary
  listener retains ownership subject to its guards. Its actual fake-socket client
  oracle covers listener removal and explicit close only. Secondary turn-lease
  release after 500 ms for accepted `running=false`, or immediately on reclaim,
  is not socket close; other owners and guards may retain it. Per-feature
  integration, admission and dynamic ordering remain incomplete.

The earlier post-91f8 parser foundation had 91 integration tests and one
incremental test pass, but was not in the frozen 91f8 ELF; its former lack of
audio embeds is superseded by the new checkpoint above. The source-only
`/tmp/hermes-rune-pinned-media-head-contract-20261030.md` records the original
Electron bearer helper's HEAD and GET behavior without returned-401 replay; it
is not media-factory end-to-end proof. Chromium method order and full player
divergence remain unknown.

The Linux dev connection checkpoint is recorded in
`/tmp/hermes-rune-connection-checkpoint-release.json`: 830 passed, zero failed,
14 ignored. The record enumerates the ignored tests and separate private runs.
It includes the SSH HTTP-header fix, bounded post-loop worker drain, generic URL
OAuth Settings and project draft-origin work. The later frozen Linux dev checkpoint
`/tmp/hermes-rune-current-linux-checkpoint-release.json` records 851 passed,
zero failed and 14 ignored (865 total), for candidate SHA-256
`abce8b7eb193fbebc4f98b8c52a7483133d1b6b6b25ae0b438e49693349d75a0`.
Native OAuth is no longer blanket pending: four private native cases are recorded
in `/tmp/hermes-oauth-native-acceptance-5/result.json`. Native source-switch OAuth
has Runtime-only evidence, and access-header embedded login fails closed with
`EmbeddedRequired`; this capability gate is not a completed embedded provider sign-in. The visible-content Search work is not yet fully accepted.

Frozen `dc2095` native editor acceptance failed Ctrl+F in both themes when the
message editor held focus (`/tmp/hermes-edit-accept-20261030/report.md`). The
subsequent source fix scopes the editor shortcut to its conversation and editor
identity; its mounted Runtime regression and foreign-input negative test passed
in the 23-test `/tmp/hermes-editor-search-final-20261030.log` run. The later
91f8 native report above supersedes that failure for the scoped editor case; it
does not establish the remaining broader search cases.

The checkpoint binary (SHA-256 `92e0e4ccd714f682d1d54888c8a00ded50033c9c72138e55f81f2b04c8271825`)
and corrected private fixture bridge (SHA-256 `080fde0f291c544878dbefcbde3282bb1676e53f3690138bda86f707a64ecd7c`)
passed Linux native SSH Stop and independent Ctrl+Q cases in both themes:
`/tmp/rune-ssh-fanout-native-03/result.json`. The accompanying
`bridge-evidence.json` records scoped interrupts and, before bridge shutdown,
retirement of each captured backend generation, lock/token/log, listener,
helper and port. The application exited successfully in all four cases.
The harness did not independently expose the interrupt acknowledgement or the
subscriber's terminal event, so those are not native wire-level claims. Its
cleanup also deleted private credential fixture roots; keyring-reference
retirement is therefore not proved by this run. Restricted `/proc` environment
access limits the broader orphan audit, although the captured process
incarnations were checked. Peer reuse also passed in
`/tmp/rune-ssh-lifecycle-native-01/result.json`; it does not establish native
ACK, subscriber wire, keyring-reference retirement or broad orphan proof. This
evidence does not establish reconnect or held-edit acceptance, nor macOS or Windows acceptance.

Projects fixture limits: normal, voice, slash and Bot creation have loopback
dispatch coverage. Equal-ID cross-source routing and stale callbacks are covered
as state/routing behavior, but a two-source outbound dispatch and a separate
NativeSelf project `session.create` loopback are not yet proved. Bounded native
abce evidence covers tree, entered-project, cwd, click and new-editor behavior,
and preview remained red at that historical snapshot: `/tmp/hermes-projects-independent-draft-fixed-abce-20261030`. The later dc2095 preview evidence is recorded above.

- An earlier registered source passed 785 Rust tests with 11 ignored in
  `/tmp/hermes-ssh-barrier-common-gate.out`. The ignored private SSH tests were
  run separately: isolated Secret Service and a fake SSH executable exercised
  resolve and commit, same-socket roster and runtime proof, credential retirement,
  and cancellation or a settings edit while proof was in flight. Further native
  SSH scenarios and macOS and Windows acceptance remain outstanding.
- The installed-package candidate 10 is an older snapshot, not the current release; it predates the current SSH changes. On Linux its extracted
  AppImage passed the normal parent-to-plugin child flow, SDK callback and child
  reuse/termination, plus actual private D-Bus notification delivery and
  source-bound activation: `/tmp/hermes-c10-installed-runtime-attempt12/result.json`.
  This is evidence for those paths, not a current release or cross-platform proof.

Historical abce subagent basic native acceptance is partial:
`/tmp/hermes-abce-subagent-acceptance-20261030-b/result.json`; advanced scenarios
remain unrun. Linux is the current native proof platform; macOS and Windows are
deferred, not verified. Local/SSH and generic URL/OAuth remain in scope; managed
Nous/Hermes Cloud are excluded.

## Evidence at this checkpoint

- The current frozen fleet integration passed **607 Rust tests**, zero failed,
  six ignored; formatting and locked offline build passed. Executable SHA-256:
  `aaf68a8f613fb04034fff00ee5d47e26f0cb80a9eae49a005ce2c514e9983897`.
- That same executable passed **3 actual native fleet scenarios / 23 checks**,
  with **14 PNGs**, real private X11/WM input and GTK clipboard reads. The cases
  cover exact B edits with A's input intact, a complete B session window with its
  captured label, actual A-to-B-to-A editor/epoch/draft restoration, presented
  streaming/partial answer and scoped Stop, 13-to-14 condensation, the hidden
  picker, both themes and known Raw/Saved display aliases. These are strict
  private protocol fixtures, not live provider or user-agent mutations.
  See `native-fleet-acceptance.md` for exact evidence and limitations.
- A genuine native startup regression made 8 automatic requests despite an
  invalid device-default file. Automatic fleet reads now wait for successful
  exact activation or explicit user refresh; the same unchanged zero-request
  failure assertion passes. The same executable passed all sixteen native
  startup cases, the complete peer-window flow with exact GTK copies and the
  rich read-only subagent miniwindow. The four native lanes produced 57 PNGs;
  the same executable also passed **97 renderer/script-input cases**: editors 5,
  composer 19, client settings 15, shell 10, feature pages 24, rich chat 13 and
  Viewer audio navigation 11. The last lane discovers its real source-qualified
  session row in one uncounted boot render; its original eleven behavior
  assertions remain intact. Failed old-key evidence is retained separately.
- Durable ninth evidence archive:
  `/home/ubuntu/Artifacts/hermes-rune-ninth-checkpoint-2026-10-30.zip`.
  Its manifest records the source commit, same tested binary, exact executed
  harnesses and all evidence hashes. This is Linux proof, not a packaged client
  or native macOS/Windows/distribution acceptance.

### Eighth checkpoint

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

### Linux MEDIA development checkpoint (2026-10-30)

The post-capture ledger entry is outside the archived source; the indexed copy of
this document is the pre-entry version. The unique marker is
`/tmp/hermes-rune-media-linux-green-20261030-emBYFLsQ.json`, and its manifest is
`/tmp/hermes-rune-media-checkpoint-20261030-emBYFLsQ/manifest.json` (SHA-256
`2ff6be20c6e1404b5a6e2f0f7963fbda1fddb86d9eedcc3ecbf6f4b7133793fe`).
The frozen tuple is marker SHA-256
`cbaf1a5bbe900832c00cc880d0486c5c5c4a6a908f5cbddcb02d53adc633f5bd`,
production ELF `8c6e131f1716ae3e0591a36f056ff45f762ef8f5abc870c02fe0f79d1a84a3b2`,
test ELF `87a61de59b625f53eb24d0f2b09bcd5e06bcea470a31e7c217db86989fec94d0`,
archive `3c732dbddf5741b9e110651ca618c522d8d56b9fc004791cc6b2d1a5a8ac2b47`
and index `02ae3c8f71356ef994bd50f33cf4bd413547c3fa74bce2b039a4ea43405f70d9`.
The verified source index contains 6,496 paths; these artifacts were independently
rehashed in the recorded evidence. Locked,
offline Linux production build and test compile passed. The captured test ELF
ran **894 passed, 0 failed, 14 ignored**; six separately selected private
SSH/OAuth cases each passed. The pinned Rune verifier and reverse/forward patch
hashes passed; the incremental library unit case and nine integration targets
passed, with **1** and **106** cases respectively. These runs were new, not reused.
`git diff --check` passed. Broad Cargo formatting and scoped Markdown formatting
failed on existing differences; full offline metadata failed because
`shared_library v0.1.9` was uncached. Logs and exit codes remain in the checkpoint.

This is bounded Linux build/test provenance, not by itself native MEDIA acceptance or full
parity. Partial-file to Audio, changed-path/late-HTTP retirement, source/profile/
editor lease fences, backfill/receipt reconciliation, actual nonaudio clicks,
image/table/typography/refresh, wider native MEDIA and other platforms remain open.
Earlier native results on the 625 binary do not establish behavior of this ELF.
Managed Nous Cloud remains excluded; generic URL behavior is in scope.


### Subsequent bounded evidence and current source limits (2026-10-30)

`/tmp/hermes-media-stream-closure-20261030/REPORT.md` and `cases.json` record
Light and Dark native live agent-file streaming on immutable 8c6e: an incomplete
quoted directive had zero players; its closing delta mounted one 20-second
player. Scoped playback produced app-PID PCM on private Pulse. The same Runtime
player path, paused position and volume survived a later append and search
rerender. The exact Unicode filename produced a visible highlight and nonzero
count in both themes; the encoded `#media:%2Fprivate%2F` alias returned zero in
both. `Update::Changed` execution is inferred from frozen source and observed
live transitions, not enum instrumentation; the Runtime path is not independent
proof of worker generation or allocation identity. This supplements historical
`/tmp/hermes-media-native-execution-20261030/REPORT.md`: filename and alias are
resolved for this live case, not all historical cases. Ordinary HTTP negative,
wider ownership and cull, durable receipt, rate, refresh and other OSes remain
unproved. The marker, five artifacts and WAV matched before/after hashes; owned
processes exited and recorded resources were released, without a complete
transient census.

The external strong-ETag native case is **not accepted**: the initial 206 was
accepted and cached and the decoder opened, but a later `FormatReader::next_packet`
returned a non-`UnexpectedEof` error whose underlying variant/message was
discarded at `decode.rs:255–256`. Exact peer replay and next/final/validator
controls passed, but no subsequent native range was observed. Neither production
nor fixture is attributed. `/tmp/hermes-range-diagnosis-20261030/REPORT.md`,
`diagnosis.json` and `replay.json`, and
`/tmp/hermes-client-boundary-20261030-956469/REPORT.md` and `diagnosis.json`
record the limits; gdb/lldb were absent and the boundary inspection made no new
native run. An earlier missing-ETag sequential/disabled-seek fixture explanation
does not explain this later decode failure. The post-freeze range regression and
per-player/generation test-only packet-error capture are uncompiled and unrun;
there is no production fix or current-tree gate. See native-audio-player-parity.md.

Create app/test `check --locked --offline -j1 --tests` passed before later tests;
static source review fixes are not runtime proof. Vendor `check --tests` stopped
with -15 for disk. Four later registered Create integration tests (including the
interleaved IDE writer) and four vendor driver-loop tests remain uncompiled and
unrun; the existing four Create model and two directory tests and Presenter
source definitions also remain unrun. Receipt/generation bug hypotheses remain
unproved and no new candidate captured Create. Full-source, regression, common
and native Create gates remain open. The stale-picker owner is still working; no
outcome is asserted here.

The two specifically approved rustc cache parents were removed; the execution
report records the completed cleanup and subsequent available space. No further
cleanup is authorized. The earlier 1.25 GiB figure was time-bound. The frozen
checkpoint above predates later dirty source and is not a new execution. Managed
Nous Cloud is excluded; generic URL OAuth remains in scope.


### Later Linux headless integration (2026-10-30)

The current Create test ELF compiled and passed 906 common tests (14 ignored),
plus six individually selected private SSH/OAuth tests. Nine Create tests and
two directory error tests passed, including real Runtime stale-picker dispatch
and interleaved IDEA writer/Create receipts. A held generation reproduced a
late idea overwrite before the submit-revision fix and passed afterward. The
external strong-validator 20-second WAV worker test passed with a null output,
query and credential checks. This does not reproduce or explain the frozen
native packet error. The old 8c6e checkpoint remains historical. Full Create
source ownership, native picker/audio and other-platform acceptance remain open.

The vendor `rune` test build initially exposed a missing portal test import;
that import was repaired and the vendor provenance verifier and reverse/forward
patch hash checks passed. The subsequent offline test build was stopped at about
1.28 GB free before linking to preserve the 1 GiB reserve. Vendor Presenter and
driver tests have therefore not run; no vendor runtime acceptance is claimed.

### Further bounded gate attempt (2026-10-30)

The previously compiled app ELF passed the exact existing NativeSelf root-proof `projects.tree` test. New cross-source Create/IDEA and NativeSelf Create authority tests are source-only and uncompiled; the earlier 906 common and six private results are for the prior ELF. The vendor test compile was stopped under capacity pressure before an ELF existed, and Presenter tests remain unrun. Both requested driver loops initialize a GPU, which this task explicitly excluded. The vendor source verifier passed. The regenerated authorized app cache was removed with a measured 2,041,167,872-byte gain; after the vendor attempt the 1 GiB disk reserve was crossed between guard samples. No new production checkpoint or full parity is claimed. See native-projects-parity.md for the remaining boundaries.

### Current RAM-backed Linux tests (2026-10-30)

A current-source test ELF passed 908 common tests (14 ignored), including the
new cross-source Create/IDEA and NativeSelf Create authority tests, and six
separately selected private SSH/OAuth cases. Four Presenter tests and four
actual offscreen tooling/desktop driver loops passed using software Vulkan.
The common suite first encountered two overlong Unix socket fixture paths and
passed after rerunning under a shorter private temp root. These volatile RAM
outputs are not a persistent checkpoint. Native chooser integration, the full
local/remote picker identity boundary, external native audio diagnosis, other
platforms and full parity remain open.

The Mermaid-enabled RuneKit gate additionally passed its selected unit test and
106 integration tests. The current production app built in the same private
RAM overlay without a UI launch; that volatile binary is not a durable release
checkpoint. Vendor verification and a reverse/forward patch round trip passed.
