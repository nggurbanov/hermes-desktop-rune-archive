# Rune port slices

The user approved a complete eventual native port, preserving placement,
navigation and behavior while using the OpenCode Rune visual system. The first
read-only increment is a starting slice, not a substitute for that migration.

1. **Native reader.** Real external gateway connection; profiles; recent visible
   stored sessions; bounded Markdown history; search/reconnect; actual light/dark
   OpenCode widgets and the Hermes Default topology. Implemented on Linux.
2. **One complete conversation.** Draft/create with source gui, native tickets,
   canonical attach and ownership, durable prepared input/admission receipts,
   streaming/replay, stop generation, reconnect/history and approval/clarify.
   Port the existing client behavior contracts; do not reuse OpenCode's fixture
   send handler or silently lose explicit launch policy.
3. **Bots and concurrent conversations.** Bot Chat's (profile, title="Bot Chat")
   registry, no stored-ID pin or recency adoption; connection/profile/session
   isolation, parallel chat views, queue, rewind, attachments and read markers.
4. **The full pane tree.** All existing presets, nested groups/splits, tab stacks,
   resize/drag/drop, persistence, focus routing, narrow windows and keep-alive.
   Rune SplitView is a primitive, not the complete Hermes layout engine.
5. **Working surfaces.** Real files/review/editors and a real PTY/VT terminal;
   rich HTML/browser previews need an explicit embedded surface integration.
   Static file trees or a painted terminal are not parity.
6. **Remaining routes and platform capabilities.** Settings, skills, messaging,
   artifacts, cron, plugins, secondary windows, voice, global shortcuts, native
   dialogs and distribution. Preserve each feature's actual behavior contract.

The current adapter is Rust and read-only. The write slice must deliberately
port canonical-protocol semantics from the retained TypeScript client; moving
the widgets does not make those semantics automatic. Do not embed the old
Electron renderer and call that a Rune port.

## Explicit framework gaps

The current Rune revision has no OS accessibility bridge, no terminal or browser
host, no React GUI-plugin ABI, and platform-specific gaps in menus, recording and
drag/drop. These require integrations or upstream work; do not silently remove
the old feature or claim parity from a tree/screenshot. Native macOS and Windows
acceptance remains separate from Linux headless tests.

## Proof per slice

Use real production APIs against controlled fixtures, keep wire ownership and
user intent, and inspect actual native renders. Real-agent smoke checks must not
restart its canonical gateway or turn read-only testing into unsolicited prompts.
Synthetic screenshots may be committed; private session contents must not be.
