# Hermes Desktop Rune development

This repository owns a standalone desktop client. It does not contain or maintain
Hermes agent execution, the Python package manager, messaging gateway, CLI, TUI,
website, or the upstream installer. The retained Electron interface is the parity
baseline for the native Rune port. Preserve placements and behavior while using
the user-selected OpenCode Rune theme and components. Structural redesign is later.

## Layout and commands

- `apps/desktop`: Electron main/preload, React UI, native helpers and client tests.
- `apps/shared`: gateway client, types and frozen protocol compatibility artifacts.
- `apps/rune`: native Rust/Rune viewer, starting with real read-only gateway access.
- `scripts/build`: JavaScript compilation, freshness and Node/Electron input helpers.
- `desktop-baseline.json`: upstream snapshot provenance and migration status.

Read `apps/desktop/AGENTS.md` and `apps/desktop/src/AGENTS.md` before changes there.
Use Node 26+ and npm 11.17+; run `npm ci` at the repository root. Run
`npm run typecheck`, relevant Vitest behavior tests, and `npm run build`. Packaging
changes also require `npm run pack` and the desktop workspace's `test:package`.
Do not provision an agent or Python environment to build or unit-test this client.
The optional external contract/E2E lanes require an explicitly selected external
Hermes installation and isolated user homes; see `apps/desktop/e2e/README.md`.

## Boundaries

- Hermes owns agent profiles, sessions, memory, tools and execution. Use the
  existing gateway APIs; do not reproduce agent behavior in the UI.
- Normal local launch invokes the installed `hermes gateway ensure --json` and
  authenticates with private, transient native tickets. Closing the client must
  leave the canonical gateway running. Remote failures never start a local owner.
- Electron owns this app's connections, preferences, plugins, logs and windows in
  its own userData. Explicit agent configuration operations remain agent-owned.
  Never migrate or delete external agent data as a desktop startup side effect.
- Renderer state must declare connection/profile/session scope. The backend is
  authoritative for state other Hermes clients can change. Preserve Bot Chat's
  `(profile, title = "Bot Chat")` identity; never use recency or a stored ID pin.
- Keep the preload capability bridge narrow. Renderer code has no direct Node API.
- Preserve generated gateway/slash snapshots with provenance. Updating upstream
  compatibility is an explicit task, never a build-time source dependency.

Prefer feature-owned nanostores, thin routes, narrow hooks, shared interfaces and
table-driven dispatch. Test user behavior and authority boundaries; do not inspect
backend source text or freeze incidental implementation details in tests. Never
write tests to a real Hermes home or use live provider credentials implicitly.
Preserve MIT attribution and third-party notices. Do not contact the user through
Hermes/Telegram for tasks started manually in T3 unless explicitly asked.

## Rune port

Use one active build environment and one persistent Cargo target at a time.
Keep the latest verified checkpoint and unique data needed to continue work.
After checking references and preserving needed data, remove unused temporary
builds, unpacked duplicates and obsolete checkpoints. Routine cleanup is already
authorized by the user. Monitor target allocation and free space before, during,
and after builds; stop before resource pressure threatens the host.

Read PRODUCT.md, DESIGN.md and apps/rune/README.md. Native Rune code is separate
from the Electron baseline. Keep Rune/runtime/kit/theme pinned to one Git revision
and commit the Cargo lockfile. Reuse upstream OpenCode components and selected
view functions with recorded provenance and notices. OpenCode transport, fixture
fallbacks, single-session IDs and its Beta storage are not Hermes semantics.
Run npm run test:rune and cargo fmt --manifest-path apps/rune/Cargo.toml --check.
Inspect actual native renders; an element-tree dump does not establish platform
screen-reader support. Never claim full parity from the first read-only slice.
