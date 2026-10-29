# Native chat migration

The native transcript uses real Rune/OpenCode Markdown and controls. Hermes owns
connection/profile/session identity and canonical gateway mutation semantics.
This is a working migration increment; complete chat parity remains in progress.

## Implemented

- Stored display rows preserve complete server payloads and durable row IDs.
  Native projection renders ordered text, Markdown/code/tables, readable thought
  disclosures, tool calls with arguments/results, image parts and file actions.
  Tool results join by tool-call identity; opaque provider replay carriers are
  not rendered as thoughts. Folded tool results advance the durable branch
  boundary through their actual source row.
- Copy, quote into the captured conversation's draft, TTS/read aloud, inline user
  edit, retry, restore to draft, and branch from a saved boundary. File/process
  actions use the existing owner-checked Files/Terminal entry points. Native
  image previews read authenticated agent media/proxy endpoints, retain owner
  keys, and decode through Rune ImageCache with bounded source sizes.
- Canonical edit/retry/restore explicitly confirm removal of the selected turn
  and later turns. The retained prepared-send journal records an exact rewind
  mutation before I/O. Its durable target is a server row ID. The following
  prompt retains the same submission identity on explicit retry. Restore-only
  emits no prompt. A typed fence refusal requires fresh user intent; an unknown
  outcome reuses the original mutation UUID and CAS fences.
- Branch persists its exact mutation before I/O, keeps unknown outcomes for
  explicit retry, and writes child receipts/definite-refusal tombstones.
  Startup recovery reads only the selected connection/profile/stored-session's
  intents and receipts. It never replays an action automatically. Profile and
  connection changes are guarded while a mutation remains unresolved.
- Load earlier history uses retained scoped workers. Latest-order pagination can
  overlap both sides of the cached tail: shared durable row IDs place new rows
  before/within/after it. Tail refresh retains backfilled prefixes only with a
  shared anchor and unchanged authority; authoritative rewind/compaction drops
  obsolete suffixes. Source cursors include hidden rows.
- A live `persisted_turn` receipt binds user/final-assistant rows before REST
  refresh. Reasoning and full tool activity payloads remain inspectable during
  the current turn. Accepted subagent events and turn starts are forwarded with
  runtime/replay/sequence/authority fences to the separate native spectator
  window manager.
- Slash discovery reads actual `commands.catalog` metadata, preserves installed
  extensions/skills, and omits declared terminal/messaging-only commands.
  Typing an absolute path is not parsed as a slash invocation. Multiline command
  arguments retain interior whitespace. Backend dispatch uses `slash.exec`,
  falls back only on a proven missing method to `command.dispatch`, and submits
  skill/send expansions through the durable conversation path. The visible
  invocation is retained separately from model-facing skill scaffolding.

## Verification

Isolated synthetic gateway socket contracts cover restore-only (no submit),
unknown rewind ACK recovery with the same UUID and fences, and the existing
conversation lifecycle suite. Four focused chat tests cover projection, opaque
reasoning carriers, tools, catalog/extension discovery and owned branch recovery;
two history tests cover moving-tail backfill and authoritative rewrites.
`check-chat.mjs` exercises native rich rendering and history-action UI against
an isolated fixture, without connecting to an agent. The `rich-chat` scene is
synthetic; its example output is not a report from a live execution.

## Remaining parity work

- Provider/Codex sidecar detail reconstruction and folding are not exhaustive;
  image analysis summaries, citation navigation and all tool-specific rich
  presentations need their original contracts and acceptance cases.
- Reactions, transcript minimap/search/navigation, full selection/quote modes,
  timestamps/display preferences and export are not fully ported.
- Rich image/video/audio/PDF/document players, precise zoom/pan, image saving,
  generated-media galleries and media retry/eviction policies remain incomplete.
- Destructive message actions currently require the canonical revision-fenced
  gateway. Legacy mutating history paths need equivalent safe acceptance.
  Structured attachments cannot yet be replayed through edit/retry/restore;
  such actions are refused before changing history, preserving the original.
- Slash local actions and all canonical policy/alias directives are not fully
  migrated. The curated fallback registry and comprehensive keyboard completion
  behavior still need parity. Errors preserve the typed command rather than
  silently dropping it. Commands with ambiguous outcomes are not auto-retried.
- Concurrent tiles, multiple independent draft windows and complete native
  subagent rich-media renderer sharing remain separate migration work.

Original contracts: `src/api/canonical-protocol.ts`, `app/chat/transcript-backfill.ts`,
`lib/chat-messages.ts`, `app/session/hooks/use-prompt-actions`, and
`components/assistant-ui/thread` under the retained `apps/desktop` baseline.
