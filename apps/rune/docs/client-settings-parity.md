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
custom themes/skins/marketplace, completion sound and native notification
permission/click routing. Rune's current public Tx surface has no OS notification
API; this increment exposes no enabled dummy notification or global-hotkey
switch. Those platform integrations need real native services and OS acceptance.

Verified 2026-10-29: all six behavior tests passed, and all 13 native interaction
cases passed with a fresh binary. The native cases verify actual transcript
geometry and bubble alpha, live reasoning folding, sidebar density, font
controls, capture/rebinding/clear/reset/cancel/conflicts, and a shifted character
still entering the draft without triggering its global action.

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
