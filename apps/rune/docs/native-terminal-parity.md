# Native terminal port

The terminal panel renders a real VT100/ANSI cell grid directly in Rune. It uses
OpenCode's existing theme tokens, monospaced text, per-cell SGR colors and a
minimum contrast adjustment. No browser terminal is embedded.

## Implemented behavior

- Explicit Start creates a local native PTY, an interactive OpenSSH PTY for a
  saved SSH destination, or a durable remote `/api/host-terminal` shell.
  Source failures never fall back to a different machine or create an agent.
- Tabs retain connection fingerprint, profile and working directory. Hiding a
  pane does not terminate its shell. Connection/profile retirement detaches
  remote viewers and releases owned native processes/readers.
- Keyboard input, application cursor/keypad modes, function keys, Ctrl-C,
  Ctrl-R, terminal Ctrl/Cmd-W routing, bracketed paste, Unicode/IME composition,
  mouse reporting, force-selection, scrollback and resize are implemented.
- Text selection supports copy, adding a quote to the captured chat draft,
  full retained-output selection, search, and explicit opening of HTTP(S)
  hyperlinks, including OSC8 labels. Copy without a selection preserves the
  shell interrupt behavior.
- The overflow menu is an overlay; opening it does not resize the shell. Tabs
  support individual close, close others/all, and explicit forgetting of an
  unavailable shell when its close cannot be acknowledged.
- Native PTY output readers are cancellable and input writes are queued.
  File uploads run separately, so staging a remote path does not freeze PTY
  input or cancellation. SSH and remote drops upload actual file bytes through
  their captured gateway/profile before inserting the returned remote path.
- Local working directories are tracked through the owned process on Linux;
  OSC7 also updates cwd. SSH supports both POSIX and PowerShell command forms.
  The explicit platform switch applies to new SSH tabs.
- Remote reconnect always uses the same durable `terminalId`. Metadata must
  match that identity; binary replay replaces the viewport and suppresses
  terminal query replies. Trimmed replay triggers an actual resize/redraw.
  Reconnect is bounded. A start whose identity was not acknowledged is never
  automatically repeated.
- Explicit remote close uses `attach=<id>&action=close` and requires matching
  closed metadata followed by normal closure. An unconfirmed close retains
  the tab and identity for retry.
- Agent process tabs are read-only mirrors, fed by scoped
  `agent.terminal.output` and `process.list` snapshots. Stream/snapshot overlap
  is deduplicated. Closing a mirror does not kill its process or resurrect the
  tab on the next poll; explicit open restores it.
- Private client state persists tab identities/cwd and bounded local ANSI
  scrollback. Restoring metadata never starts a new process. Remote shells
  restore detached with their durable identity and authoritative server replay.
  Synthetic scenes neither load nor save real terminal state.

## Integration API

`Terminal::new/init`, `set_owner(Connection, DraftScope, cwd, ssh)`, `poll`,
`action`, `rehome`, `close_connection` and `close_profile` own lifecycle.
`view::terminal::panel/on_key/on_ime` render and route focused input.
`agent_event`, `sync_processes` and `open_process` supply process mirrors.

Conversation updates expose owned raw agent-terminal events. The existing
scoped page worker performs `Conversation::process_request` reads, preserving
runtime identity and rejecting stale owner responses. Folder changes use
`Conversation::set_cwd` on the retained conversation worker, with a retained
canonical request ID, revision/generation fences, busy denial and authoritative
normalized readback. They never become a fresh generic PageRPC mutation.

## Evidence and limits

The focused terminal suite verifies real Linux PTY input, kernel resize, ANSI
output and exit ordering; controlled real WebSocket metadata/replay/reconnect
and close receipts; parser behavior, Unicode scrollback/search, read-only
process mirrors and synthetic isolation. Conversation contracts separately
verify the normalized session-owned folder mutation/readback without a prompt.

No agent process was started or modified during these tests. Live SSH was not
used: SSH argument construction, quoting and source selection are covered by
synthetic tests. macOS PTYs, Windows ConPTY/PowerShell and actual SSH host/device
acceptance remain unverified in this Linux environment. The native code uses
portable-pty and OpenSSH rather than agent installation or lifecycle commands.
