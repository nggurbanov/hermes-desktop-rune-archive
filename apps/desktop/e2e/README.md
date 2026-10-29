# Desktop integration tests

The desktop repository does not supply or provision a Hermes agent. Unit tests
run with Vitest and never activate the Python package manager.

Local and remote integration tests use a separately installed Hermes release.
Select its checkout and prepared Python interpreter with the existing runtime
overrides before running Playwright:

```sh
export HERMES_DESKTOP_HERMES_ROOT=/absolute/path/to/external/hermes-agent
export HERMES_E2E_PYTHON=/absolute/path/to/prepared/python
npm run test:e2e --workspace apps/desktop
```

`HERMES_DESKTOP_HERMES` can select an installed launcher for app-only tests.
Tests that seed history or start a second backend directly also need the root
and interpreter above. Record the external installation's commit with test
results; protocol snapshots currently derive from Hermes commit
`ef75bc708867e3884cb40a91363be6dc6fddc034`.

The shared fixtures create separate HOME, USERPROFILE, HERMES_HOME and desktop
user-data directories. They strip provider credentials and inherited agent
session settings. Never run a fixture against a real user Hermes home.

The core suite includes independent URL-connected backends and gateway attach
tests. Their agent processes come from the selected external installation; the
desktop application and its renderer come from this repository's build.

## External runtime contracts

`npm test` runs desktop unit tests without importing server Python modules.
Python/native UI-preference parity, bootstrap import isolation, published
source-launcher behavior and native ticket-bridge authentication live in the
explicit external-runtime contract lane:

```sh
export HERMES_DESKTOP_HERMES_ROOT=/absolute/path/to/external/hermes-agent
export HERMES_E2E_PYTHON=/absolute/path/to/prepared/python
npm run test:external
```

Run that command from the repository root, or use `npm run test:external` from
`apps/desktop`. The command fails before running tests if either selector is
missing or points into the desktop repository. It never creates a Python
environment for the desktop checkout. Both PM contracts copy the selected
external source into a disposable sandbox before modifying any package-manager
state. The source-launcher fixture needs `uv` on PATH and uses a local wheel offline;
the bootstrap-import contract separately acquires ruamel.yaml and python-dotenv
into a disposable target directory. Record the selected Hermes commit and the
contract results together: an older external runtime may lack the required API.

Typecheck this lane with `tsc -p tsconfig.external.json --noEmit` from
`apps/desktop`.

The inherited installer/update Playwright lane still describes the original
monorepo's installation semantics. It is migration work, rather than proof of
standalone compatibility. Do not delete its coverage or mark it skipped to
make an extraction pass.
