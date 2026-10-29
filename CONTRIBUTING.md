# Contributing

This fork maintains the standalone desktop client. Agent behavior belongs in an
external Hermes installation. Read `AGENTS.md` for ownership boundaries and
`apps/desktop/AGENTS.md` for interaction and state invariants.

Install with Node 26+ and npm 11.17+ using `npm ci`. Before submitting a change,
run `npm run typecheck`, relevant tests (`npm test` for the full client suite),
and `npm run build`. Package changes also need `npm run pack` and
`npm run test:package --workspace apps/desktop` on the target platform.

Keep changes scoped. Include a behavior regression test for changes at connection,
profile, filesystem or persistence boundaries. The default suite must not import
or provision backend sources. External integration tests use the explicit lane
in `apps/desktop/e2e/README.md` and temporary user homes.

Explain the resulting behavior and verification in pull requests. Preserve the
upstream MIT attribution. Rune parity and later UX redesign are separate stages;
do not combine interface redesign with extraction fixes.
