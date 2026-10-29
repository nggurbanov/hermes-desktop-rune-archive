# Native Settings parity

The native page uses actual Rune/OpenCode Kit components and the OpenCode themes.
`apps/desktop/src/app/settings` and its retained API contracts are the behavior
source. This file records implemented scope, not a claim that the entire desktop
migration is complete.

## Implemented agent-owned surfaces

| Surface | Native behavior and authority |
| --- | --- |
| Config groups | Every backend-schema field plus existing config leaves, original field copy/options/grouping, booleans, enums, numbers, lists, text and structured advanced values. Sparse serialized `PUT /api/config`, debounce, explicit save/revert/defaults, readback preserving newer or invalid drafts. |
| Settings profile | Override independent of active conversation; requests and callbacks retain connection/profile ownership; dirty settings, credential drafts, writes and authentication block an owner switch. |
| JSON import/export | Native dialogs. Import merges only provided keys. Export is config only; environment credentials remain separate. |
| Model | Live catalog, main assignment through `/api/model/set`, backend confirmation retry, auxiliary and plugin task routes, effort inheritance/reset, preserved drafts during unrelated readback. |
| Mixture of Agents | Presets, duplicate/add/delete/default, references and aggregator, enable flags, temperatures, timeout, degraded-reference policy. Unknown keys, per-slot effort and fanout remain intact. Incomplete/recursive slots cannot persist. |
| Subagent routing | Dedicated backend availability, blocked reason, named model route CRUD/default and renamed `original_name`, profile allowlist/descriptions/default. Opaque endpoint, credential marker and fallback fields remain intact. |
| Providers | Real account inventory, device/PKCE OAuth start/status/submit/cancel, explicit native browser action, disconnect; custom endpoint CRUD/validate/activate, saved-key preservation and discovered models. |
| Keys | Agent-side masked environment inventory, explicit reveal/hide, replacement/add and confirmed removal; messaging-managed variables stay on Messaging. |
| Memory | Real built-in sizes/active-provider readiness, every declared provider field (including private fields), sparse typed saves, secret masking, connection start/status/poll/timeout. |
| Vault | Metadata-only list; add login/payment/address with valid website origin; confirmed local-item delete; external-manager enable/lock/unlock. Master password and submitted secret editors clear immediately. Independent RPC results must acknowledge the actual change. |
| Billing | Real account/payment method and usage; stable idempotency across uncertain credit-purchase retries; real settlement polling; typed refusal/recovery; existing auto-reload edit/confirmed disable; keep-going approval and exact switch replay; remote-spending verification event; plan preview/confirmed scheduled change/undo, portal upgrades. |
| Web access | Local connection gate from original client, saved config versus serving state, LAN/public/local addresses, actual start/stop/restart, 35-minute build request budget, restart PID failure and 90-second applied-config verification. |
| Managed local models | Active-agent profile binding, runtime install/update/server control, hardware/catalog, recommended quickstart, downloads, library activate/eject/delete, real jobs with pause/resume, HF search/file groups/download, existing agent GGUF path and local native picker; a quickstart observed running opens a new chat on confirmed completion while its panel stays active. |
| Plugin settings | Settings discovers declared schema pages. It hands the exact profile/canonical key to the shared native Capabilities editor, including its serialized credential transaction. |
| Voice catalog | Live scoped ElevenLabs account voices with labels and manual custom voice IDs; refresh never replaces the current draft. |
| Tool setup links | Workspace, Browser and Advanced open the same scoped terminal readiness/backend selection, browser consent, and Computer Use health/grant surfaces as Capabilities. |

Client Appearance, shortcuts and About are rendered by the root-owned native
client preference view, without agent profile/save controls. Gateway registry,
SSH/Cloud connections and OS keychain storage are native-client services rather
than backend config fields; their root-owned integration is separate.

## Remaining details

- External-CLI provider sign-in displays the exact agent-host command. An
  interactive native terminal service must execute it; the button does not
  pretend to have authenticated.
- Mixture-of-Agents and specialized provider forms use explicit saves; schema
  fields retain debounce autosave.
- Archive/session preferences require the shared native session manager. Generic
  backend configuration alone does not implement archived-session restore/delete.
- Existing desktop JavaScript plugin-contributed Settings pages require the
  separately owned native plugin host; agent-declared schema forms already work.
- Rune's semantic tree does not itself establish an OS screen-reader bridge.

## Verification

Behavior tests use synthetic backend results and never write a live agent.
They cover sparse writes/readback, invalid drafts, profile pinning, secrets,
OAuth lifetime, asynchronous writer independence, model confirmations, MoA opaque
fields, memory configuration, Vault acknowledgements, billing idempotency and
settlement, Web access restart proof, runtime jobs and delegation routing.

Native screenshot fixtures support:

```
hermes-rune --scene settings --settings-section memory --theme light --size 1440x900 --snap /tmp/settings-memory.png
hermes-rune --scene settings --settings-section vault --theme dark --size 1440x900 --snap /tmp/settings-vault.png
hermes-rune --scene settings --settings-dialog vault-add --theme light --snap /tmp/settings-vault-add.png
```

`--settings-section` accepts the actual page IDs. `--settings-dialog` also accepts
`vault-unlock`, `provider-oauth` and `billing-quote`. These are synthetic scenes;
no fixture action falls back to a live agent.

The native acceptance command is `xvfb-run -a node scripts/check-pages.mjs`
from `apps/rune` on Linux after a fresh Cargo build. It uses a private temporary
home and synthetic, read-only scenes. The checks exercise feature initialization,
short-window bounds, each Bot tab, Pairing/Webhooks/Telegram navigation,
Artifact filtering and Review's virtualized diff after scrolling. They complement
controller/transport tests; they do not establish provider or operating-system
acceptance on real accounts.

Verified 2026-10-29: 101 page-controller tests, 12 Review tests and 10 Command
Center tests passed. The fresh native feature-page matrix passed all 24 cases.
The separate render sweep also produced each of the 20 Settings sections, six
Bot sections and four Command Center sections. Both 1440×900 light and
1024×520 dark feature viewports are covered; real account and platform acceptance
remains separate.
