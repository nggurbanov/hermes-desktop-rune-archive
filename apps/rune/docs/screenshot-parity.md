# Screenshot comparison and remaining migration work

Audit date: 2026-10-29. Baseline: native reader commit `694900c4b9` and the
retained Hermes Desktop sources. The user accepted the corrected direction as
"almost" and supplied additional references. This is an inventory of observed
gaps and implementation contracts, not a claim of completed parity.

## Reference authority

- **Hermes S1**, attachment `1d277be5-9b51-47df-9dc6-05d36f70c874`: full macOS
  window with its transcript and session titles redacted. Establishes the
  visible sidebar sections, window controls, conversation tabs, timeline rail,
  composer controls, profile/fleet rail and statusbar. Redacted content does
  not establish message rendering or unseen menus.
- **Hermes S2**, attachment `401a6c02-6667-4aa4-ba57-6355ff7567d7`: closeup of
  the voice cluster. The microphone is the inline dictation hub; the speaker
  and wake-word ear expand vertically above it. It is not just one mic button.
- **Later T3 Code references**: attachments `75bd3f02-4ea5-4b0a-819c-45029a711503`
  and `0b325575-1ec6-416d-9b27-fc1b5f131911` show model/provider and favorites
  pickers; `af28e461-f3a6-47ff-9fb2-b87d32b581e3` shows the multiline composer,
  attachment chips and bottom control row; `7b1a0ebf-9640-4aaa-b47c-3c150eb6089e`
  shows reasoning and service-tier choices; `8e5fd8d9-4a8e-49f1-a6bb-988c5f27c7a2`
  shows permission choices. The user's later instruction explicitly makes
  these the visual/interaction reference for the new composer and model picker.

Keep the supplied private images in the conversation. Do not copy them, their
redacted transcript, or private session data into repository fixtures. New
committed screenshots must use synthetic content.

Hermes remains authoritative for navigation, agent/session ownership and
backend behavior. T3 Code supplies the explicitly requested composer direction;
its provider names, permission vocabulary and available options are not a
Hermes capability contract. OpenCode continues to supply native components and
themes. A screenshot is not permission to fabricate unsupported operations.

## Sidebar gaps

All rows below remain migration work unless a later implementation records
specific verification. Source paths are relative to the repository root.

| Visible element | Contract and source owner | Required next result |
| --- | --- | --- |
| Pinned section and empty hint | `app/chat/sidebar/index.tsx`, `session-index.ts`, `store/session-pin-sync.ts` under `apps/desktop/src`. Pinned rows precede ordinary sessions and disappear from their other source sections. Resolve durable lineage roots and live tips. Backend `sessions.pinned` is authoritative; the local list supplies ordering. | Read and show existing server pins, including rows back-filled beyond list limits. Add pin/unpin only with the real scoped mutation path. The screenshot's Shift-click gesture must not become a local-only fake pin. |
| Chronological dividers | `apps/desktop/src/lib/session-date-groups.ts` and `lib/time.ts`. An unlabelled recent activity cluster precedes coarse calendar ranges such as Last week, Earlier this month and month names. The first rendered group is unlabelled; branches travel with parents. Local day rolls over at 04:00 and week boundaries follow locale. | Preserve grouping and collapse behavior rather than inserting decorative headings or assigning each row an arbitrary age bucket. |
| Relative row ages | `apps/desktop/src/app/chat/sidebar/session-row.tsx`, `lib/time.ts`. Use `last_active || started_at`, in Unix seconds. Floor nonnegative elapsed minutes/hours/days; under a minute reads `now`. | Retain numeric timestamps in the native DTO. Do not silently lose ages because a value is not an ISO date string. |
| Leading status dots, activity arcs and unread state | `apps/desktop/src/store/session-dot-state.ts`, `session-unread-remote.ts`. The shared map resolves attention, live work/stall/background delegation, unread and draft/idle with lineage aliases. | Use confirmed state with the original priority. `is_active` is a 300-second activity window, not proof that the agent is Working. Persisted `unread` can be read without inventing live execution state. |
| Sessions filter button | `apps/desktop/src/app/chat/sidebar/filter-menu.tsx`, `store/layout.ts`. Controls grouping, ordering, row metadata, status/profile/project/PR filters, archived visibility and All profiles. | Add supported view controls with working selection, reset and persistence. Do not equate a search box or session count with the filter menu. Broader options need their real data. |
| Telegram and API sections | `apps/desktop/src/app/chat/sidebar/index.tsx`, `lib/session-source.ts`. Independent collapsible source sections with icons, recency order and load-more state. API's wire source is `api_server`; Telegram's is `telegram`. Pinned rows move into Pinned. | Read the messaging slice independently, preserve each row's owner and show sections from actual returned data. Do not hardcode these two platform headers merely because this user's screenshot contains them. |
| Cron Jobs section | `apps/desktop/src/app/chat/sidebar/cron-jobs-section.tsx`. This is a list of scheduled jobs, not a list of sessions whose source happens to be `cron`. Jobs expose recent run sessions, next-run/state information and management/trigger actions. | First implement scoped job/run reading and honest empty/error states. Trigger, pause, resume and delete remain writes requiring the actual operation. Preserve Advanced-mode visibility. |
| Profile/fleet rail | `apps/desktop/src/app/chat/sidebar/profile-switcher.tsx`, `fleet-rail.ts`. All/default home control, profiles, gateway groups, new/import/manage controls and gateway discoverability; a condensed picker replaces many squares. Background profile attention is named accessibly. | Preserve source/profile identity and the default-versus-All behavior. Current native initials and one chevron are only a subset. Creation/import/management and multi-gateway routing remain pending. |

### Data and ownership seams

The current native `Session` shape of id/title/displayed date is insufficient
for this sidebar. The read model needs owner profile/connection, source,
last-active/started timestamps, lineage identity, pin/read state and the
explicit data required by enabled filters. UI collapse/order preferences are
client-owned; agent session flags are not.

- `apps/desktop/src/api/sessions.ts::listSidebarSessions` uses
  `GET /api/profiles/sessions/sidebar` with `recents_profile`, `recents_limit`,
  `cron_limit`, `messaging_limit`, `recents_exclude` and `messaging_exclude`.
  It returns independent `recents`, `cron` and `messaging` slices, truncation
  and per-slice/per-profile error information. The older compatibility path
  uses `/api/profiles/sessions` with profile and source/exclude-source filters.
- Preserve independent limits. Taking one flat recent page and then grouping
  by source can let scheduler or messaging bursts consume the page and hide
  ordinary conversations. Preserve server pin back-fill beyond limits.
- `apps/desktop/src/api/cron.ts` reads `/api/cron/jobs?profile=P` and each job's
  `/api/cron/jobs/{id}/runs`. A run remains owned by the selected gateway and
  profile; it must not fall back to a same-named local session.
- Pin ordering cache `hermes.desktop.pinnedSessions` is connection-scoped and
  explicitly excludes profile from its storage key (`store/layout.ts`). Its
  durable truth is the owning backend row. Pin/unpin uses
  `api/sessions.ts::setSessionPinnedRemote`, a fenced PATCH carrying the owning
  `body.profile`; optimistic writes must defeat stale list replies and recover
  visibly on failure. A read-only release can show pins without enabling writes.
- Canonical Bot Chat exclusion and fresh named-registry resolution still apply
  to every new slice. Sidebar enrichment must not reintroduce hidden canonical
  chats or let recency choose a bot's relationship.

## Window chrome, timeline and statusbar gaps

| Visible element | Source owner and behavior | Status |
| --- | --- | --- |
| New conversation plus beside workspace tabs | `apps/desktop/src/components/pane-shell/tree/renderer/tree-group.tsx` and workspace/session contributions. Creates a draft in the intended pane rather than only painting a plus. | Pending real draft/create wiring and tab placement. |
| Settings, layout editor/reset, HUD, pane flip and right-side toggle | `apps/desktop/src/app/shell/titlebar-controls.tsx`, `store/titlebar-app-actions.ts`. Settings/Layout/HUD default to the right; sidebar remains left; flip and right-side toggles are positional. Layout modifier-click resets. Simple mode filters instrumentation, not Settings/Layout. HUD depends on host capability. | Native global controls are only a subset. Complete the actions with the pane/window model; a disabled affordance is not a transferred feature. |
| macOS traffic lights and system menu bar | Native window/platform ownership. | Native macOS acceptance is separate. Do not reproduce the screenshot's system menu bar or fake traffic lights inside Linux application content. |
| Right transcript tick rail | `apps/desktop/src/components/assistant-ui/thread/timeline.tsx`, `timeline-rail.tsx`, `timeline-index.ts`. Represents prompt positions, hover previews and active position; keyboard Up/Down/Home/End navigate; clicking reveals the matching prompt in this conversation, including an unloaded historical window. Hidden panes do not measure or subscribe. | Pending. The current bounded Markdown reader is not this navigation surface. |
| Bottom gateway/profile, backend health and workspace | `apps/desktop/src/app/shell/hooks/use-statusbar-items.tsx`. Gateway and fallback profile selectors, health detail and workspace cwd follow the focused conversation's owner. | Native endpoint/reconnect text covers only a subset. No fake Backend ready or local cwd for a remote conversation. |
| Bottom approval mode, client/backend versions and other readouts | Same statusbar owner, `shell/approval-mode-menu.tsx`, `store/approval-mode.ts`, version helpers. Client facts come from the app; runtime version/config comes from the selected backend. | Pending capability-backed readouts and menus; screenshot values must not be hardcoded. |

The timeline index API is `GET /api/sessions/{id}/timeline?limit=500`, scoped by
profile/owning connection and paged with `after_row_id`. It returns prompt
`row_id` and preview metadata without downloading assistant/tool bodies. A
native port needs durable message/row identities and historical-window reveal,
not evenly spaced decorative marks over the currently loaded 500 messages.
Cancel stale jumps when the session or pane changes.

## Composer: implemented subset and remaining behavior

The compact Hermes composer in S1 and the voice fan in S2 establish missing
functionality. The later explicit T3 Code direction supersedes copying the
old composer's exact visual layout. The native composer/model-picker subset
below is **implemented and verified within the stated limits**. It remains a
local draft surface; sending, agent configuration changes and native audio
are not part of this completed subset. Sidebar, chrome and timeline gaps above
remain pending.

Native owners are `apps/rune/src/composer.rs`, `src/view/composer.rs` and
`src/gateway/catalog.rs`. Drafts are keyed by connection/profile/session;
favorites are keyed by connection/provider/model. Both are currently in-memory
and are lost when the application exits. Selecting a model or settings stages
the next draft; it does not change past history or confirm the active runtime.

The later voice references add the empty-composer waveform action and separate
engine chevron, now transferred with profile-owned read-only status. Voice mode
is backend configuration rather than a draft setting. See
[voice-parity.md](voice-parity.md) for the complete audio/engine contract, and
[lower-left-parity.md](lower-left-parity.md) for the source-backed inventory of
every lower-left rail, nested menu, Command Center and statusbar feature.

| Surface | Implemented and verified subset | Remaining semantic boundary |
| --- | --- | --- |
| Multiline editor and attachments | T3-directed multiline editor, bottom controls, removable local-file tiles and supported image previews. The attachment action invokes Rune's native file chooser, retains the draft's owner and reports file errors. | Draft persistence, full IME acceptance and actual attachment preparation/upload remain pending. The scripted chooser result was tested; the real desktop portal was not exercised. A local preview is not an uploaded attachment. |
| Model/provider picker | Real profile-scoped `model.options` read, with same-owner `GET /api/model/options?profile=P&explicit_only=1` recovery. Provider and Favorites filters, autofocus search, selection check, star toggling, keyboard navigation/selection and model shortcuts work. Catalog loading/errors remain separate from conversation history. | Favorites currently last only for this process. Unknown/unavailable entries are not silently replaced with reference-image models. Applying the selection to a real submitted turn remains pending. |
| Reasoning and service tier | Separate staged effort and tier choices. Reasoning, thinking-off, Fast and Ultra Fast are gated by the catalog's `reasoning`, `can_disable_reasoning`, `fast` and `ultrafast` flags. Incompatible staged choices are cleared on model changes. | The catalog advertises support flags, not route-specific effort ranges or current effort/tier values. The staged effort vocabulary does not prove every provider accepts every level. Backend application/validation and confirmed runtime readback remain pending. |
| Permissions | Picker uses Hermes values `manual`, `smart` and `off` plus Agent default; labels are Manual, Smart and Full access. Choices are staged only in the scoped draft. | No `config.set` or approval-policy mutation occurs. The retained `store/approval-mode.ts` owns profile-scoped `approvals.mode` through `config.get`/`config.set`; T3's four labels must not become invented Hermes modes. Actual application and readback remain pending. |
| Send, stop, queue/steer and voice primary | Send is explicitly disabled and the composer says it is draft-only. Local text/settings do not create an agent turn. | Durable submission, streaming/replay, stop ownership, queue/steer, approval/card answers and recovery remain pending. `apps/desktop/src/app/chat/composer/controls.tsx` defines the distinct idle/running/typed/waiting states to transfer. |
| Dictation, spoken replies and wake word | The inline mic cluster reveals separate speaker and wake-ear controls vertically above it; hover/open and Escape dismissal are verified. All audio actions are explicitly disabled. | Native capture, playback, wake-word integration, permissions and truthful independent audio states remain pending. `apps/desktop/src/app/chat/composer/voice-fan.tsx` and `components/ui/fan-menu.tsx` remain the behavior owners, including the narrow/HUD `VoiceMenu` variant. |

Verification reported for this increment: 34 Rust tests and the prior 10 native
shell scenarios passed. The 12 new scripted native composer scenarios also
passed: editor/control placement, search autofocus, provider/Favorites filters,
keyboard selection (including physical Enter after filtering) and shortcuts,
staged settings, voice expansion/Escape, scripted file selection and a short
`1024×520` dark window. An actual Linux X11 window was also exercised for search,
keyboard/mouse selection, Escape and minimum-window resize. Explicit instant
commits avoid stale native pixels when a model selection closes the popup.
These presentation and input checks use synthetic data;
they do not establish real portal interaction, provider turn execution, native
audio or platform screen-reader support.

## Actionable continuation slices

1. **Read-only sidebar data.** Extend the native DTO and read transport with
   independent scoped sidebar slices, pins, sources, timestamps and lineage.
   Prove server pins remain reachable outside the recent limit, sibling slice
   failures remain distinct, and profile/connection switches reject stale data.
2. **Sidebar presentation and view controls.** Add pinned/date/source/job
   sections, relative age, confirmed status markers, collapsing, supported
   filters and the remaining rail anatomy. Verify populated, empty, collapsed,
   loading/error, long-list and narrow-window states with synthetic data.
3. **Real session flags and full rail actions.** Add fenced pin/read-state writes
   and their optimistic reconciliation, then creation/import/management and
   fleet routing. Prove same-id rows on different profiles never cross-write.
4. **Complete conversation beyond the staged composer.** Preserve the verified
   editor/picker subset while adding durable draft/favorite storage and actual
   capability-backed send/stream/stop, attachment preparation and approval
   policy application. Exercise the real chooser portal and provider paths;
   keep unsupported voice/provider/permission capabilities explicit.
5. **Timeline and complete chrome.** Port stable-row prompt navigation and
   historical reveal, then remaining window/pane controls and real status
   readouts. Verify owner isolation, stale-jump cancellation, hidden-pane
   idleness and native platform behavior.

These slices extend the full migration; they neither remove earlier scope nor
turn the screenshot into a substitute for behavior contracts. Mark an item
implemented only with its concrete working state and verification evidence.
