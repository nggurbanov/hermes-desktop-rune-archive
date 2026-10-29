# Rune port slices

The approved target is full functional parity with the retained Hermes Desktop,
using a standalone native Rune client and the OpenCode visual system. Preserve
Hermes placement, navigation, identities and behavior. The updated 21:12 T3
composer reference is the explicit layout exception. Full parity remains open;
[functional-parity.md](functional-parity.md) records verified current evidence.

1. **Standalone reader and boundary.** Existing local, URL and SSH agents;
   scoped profiles, canonical Bot Chat, recents, history and both appearances.
   Implemented on Linux. The client contains no Python agent core and does not
   start an agent as a side effect of connecting. OS distribution and additional
   platform discovery still require acceptance.
2. **Complete conversations.** Native prepared input, attachment upload,
   canonical/legacy admissions, UUID receipts and unknown-outcome recovery,
   streaming/replay/stop, queue/steer, approvals/clarification, workspace,
   multimodal rewind/retry/restore and branch are implemented. Model, effort,
   service tier, profile approval and independent session bypass follow actual
   authority and retained user revisions. Provider-specific presentations and
   remaining commands/interactions are tracked in native-chat-parity.md.
3. **Bot Mode and concurrent conversations.** Profile/SOUL/model/assets,
   archive/import/setup, canonical Bot Chat and bundled Bots SDK groups are
   implemented. Complete fleet integration, simultaneous interactive tiles,
   peer windows and multiple-window state reconciliation remain required.
4. **Pane tree.** Original presets and named layouts, nested groups/splits,
   native docking/tab stacks, divider resize, collapsed rails, visibility,
   simple/advanced restoration and persistence are implemented. Dynamic
   session/preview/route tiles, plugin geometry and their focus/ownership need
   completion. A split primitive alone is insufficient.
5. **Working surfaces.** Native Files/editing, Git Review and PTY/VT Terminal
   controllers are implemented, with private repository/PTY contracts. Shared
   image originals, fit/zoom/pan/save and authenticated streaming are present.
   PDF, audio/video, interactive HTML, watcher behavior and remaining external
   host/platform interactions still need implementation or acceptance.
6. **Routes and platform capabilities.** Settings, Capabilities, Messaging,
   Artifacts, Scheduled jobs, Command Center, rich child spectator windows,
   voice, original React GUI SDK and client preferences have substantial native
   implementations. The current increment finishes completion cues, background
   process status and plugin notification callbacks. Remaining media, Cloud,
   global shortcuts/HUD, haptics, OS accessibility and macOS/Windows packaging
   stay on the complete parity checklist.

Native Rune owns the application shell. The separately owned WebView process
hosts the original React GUI-plugin ABI; it does not turn the old Electron
renderer into the main application. Rich browser/HTML surfaces need a distinct
unprivileged lifecycle, not a raster substitute or the privileged plugin host.

## Proof per slice

Use production code against controlled peers, preserve exact wire ownership and
user intent, and inspect actual native renders. Screenshots use synthetic data.
Read-only checks against existing user agents never become unsolicited prompts,
configuration changes, installations, restarts or microphone/provider calls.
Linux tests do not establish macOS/Windows behavior. The functional ledger keeps
these distinctions and the remaining framework/platform gaps explicit.
