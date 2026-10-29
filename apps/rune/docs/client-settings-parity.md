# Native client preferences

Source contracts are the retained Hermes renderer stores and Settings pages,
including `chat-text-scale`, `session-list-density`, `reasoning-disclosure`,
`tool-view`, `user-bubble-transparency`, `keybinds`, `display-timestamps`,
`chat-font-setting` and `terminal-font-setting`. This increment does not claim
complete desktop parity.

| Preference | Authority and native behavior |
| --- | --- |
| Theme, mode, rail/status/sidebar/tool visibility | Existing native device preferences and tree-aware pane actions. Theme shortcuts resolve the actual system appearance before choosing an explicit opposite. |
| Conversation size | Device-only presets 90/100/110/125/150/175, with 110 as the original default. Prose, headings, code and inline edit typography change together; the native port's default reading size is retained. |
| Session density | Device-only Compact/Comfortable/Detailed; actual row heights and detailed preview metadata change. Existing card/list and metadata preferences remain independent. |
| User bubble transparency | Device-only 0–100%; the actual theme fill alpha changes while text remains readable. |
| Reasoning/tool disclosure defaults | Device-only collapsed reasoning and technical tool detail preferences. Simple mode shadows them without overwriting them. Per-conversation explicit open/close choices outrank the default. |
| Font families | **Agent profile config**, matching the original `desktop.font_family` and `terminal.font_family`. Existing Settings drafts, sparse autosave and confirmed readback own the write. New code does not mirror these into device storage. Inter, IBM Plex Mono and the installed system monospace face are actual Rune choices; unsupported saved families remain intact and use the bundled fallback. |
| Reasoning visibility/timestamps | **Agent config**, including the original CLI-platform reasoning override. Read-only cached config is rebound by connection/profile and refreshed; local presentation changes never write those keys. |
| Workspace directory | Existing native directory picker and retained conversation workspace change; a new draft stages its directory. The UI distinguishes the current conversation from the independent agent Settings profile. |
| Keyboard shortcuts | Device-only overrides for implemented native actions; capture, Escape cancel, Backspace/Delete unbind, per-action/all reset, conflict names and deterministic dispatch. Empty overrides stay unbound. Unknown late/plugin IDs survive ordinary writes and reset of built-ins. Physical shifted keys are independent of the emitted symbol. Bare/Shift-only bindings do not take keys from focused controls. Terminal and model-picker routing retain focus priority. |

Device changes save only after an explicit control action to private
`display.json` in the standalone client state directory. Unsupported/newer or
unreadable saved files are preserved and become read-only instead of being
silently overwritten. Fixture actions change their synthetic view only and do
not write user or agent state.

## Acceptance and remaining parity

Six behavior tests cover durable private device state, unknown key preservation,
malformed/newer file protection, clear/reset/conflict dispatch, physical key
identity and typing safety, profile read isolation, and clean/sparse font drafts.
The native interaction script is `scripts/check-client-settings.mjs`; it uses an
isolated temporary home and actual Rune controls/keyboard events.

Full parity still requires the remaining original actions and contributed
plugin keybindings, original cloud/fleet/global window actions, arbitrary
installed/custom font handling, native zoom/text direction, translucency,
custom themes/skins/marketplace, completion sounds, complete native notification
coverage and global hotkeys. The separate Linux notification adapter and its
remaining platform/visibility limitations are documented in
`native-notifications-parity.md`; these preferences keep their device authority.

Verified 2026-10-29: all six behavior tests passed, and all 15 native interaction
cases passed against the immutable third-checkpoint binary. The native cases verify actual transcript
geometry and bubble alpha, live reasoning folding, sidebar density, font
controls, capture/rebinding/clear/reset/cancel/conflicts, and a shifted character
still entering the draft without triggering its global action. Recording also
survives an attempted click on the disabled search field and cancels when leaving
the visible shortcut editor. Evidence: `/tmp/hermes-client-settings-third`, with
`/tmp/hermes-client-settings-third.log`.

Run from `apps/rune` after building:

```sh
cargo test --locked client_settings::
xvfb-run -a node scripts/check-client-settings.mjs
```

Additional original presentation controls remain open, including code-diff and
timeline presentation, model-pricing labels, intro/composer-popout/backdrop
preferences, embed consent and the profile-scoped reaction/tip/tour tool mirrors.
The original mirror contract writes only explicitly touched preferences on a
connection, so an initial default must never overwrite an independently managed
agent's configuration.


## Shared preference writers

Main device/layout preferences, display/shortcut settings, profile overlays and
sidebar preferences now save only changes from their last successful local
snapshot under an OS file lock. The transaction re-reads and validates the latest
file, recursively merges changed JSON fields and atomically commits privately.
Unrelated/unknown fields survive a stale writer. Arrays and sets are one atomic
field; this does not promise merging individual set members.

Explicit display/sidebar choices carry touched field paths. Reset, Clear and
selecting an already displayed value therefore apply the user's intent even
when another writer has changed that value on disk. Shortcut reset removes only
its named known binding, retaining unloaded-plugin bindings. Failed saves retain
both local edits and touched paths for a scheduled retry. A malformed/future file
is preserved; an initial failed load grants no write access. Profile removals
also name their key explicitly, including a profile absent from a stale snapshot.

Display/sidebar controls adopt the merged preferences and baseline together.
Main-window layout/appearance and profile overlays retain their local successful
snapshot as the diff baseline; otherwise the next UI poll would echo another
window's stored geometry or appearance back into the file. There is no automatic
live observation of other windows yet. Independent WindowId, per-window geometry,
connection-registry transactions and admission ownership remain separate work.

The frozen integration passed **439** default tests with zero failures and three
ignored private entry points. The two storage helpers are exercised by default
parent tests launching actual child processes; the physical-output-free private
audio case remains explicitly exercised at the previous checkpoint. Shared-state
contracts cover stale writers, the OS lock, nested removal, unknown/schema
preservation, forced unchanged resets and failed-save repair/retry. Focused logs:
`/tmp/hermes-shared-client-tests.out`, `/tmp/hermes-shared-display-tests.out` and
`/tmp/hermes-shared-sidebar-tests.out`. Parent default log:
`/tmp/hermes-goal-six-tests.out`.
