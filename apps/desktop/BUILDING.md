# Building Hermes Desktop Rune

Use Node 26+ and npm 11.17+ at the repository root.

```sh
npm ci
npm run build
npm run pack
npm run test:package --workspace apps/desktop
```

`pack` creates an unpacked application under `apps/desktop/release`. The package
check verifies its identity, standalone stamp, renderer and native terminal
binding, and rejects an embedded agent payload. It does not launch an agent.

The build includes Electron, the renderer, native desktop helpers, and client
compatibility snapshots in `apps/shared`. No Python source, interpreter, agent
dependencies or agent installer belongs in a desktop release. A local connection
uses a separately installed Hermes; URL and SSH connections use external runtimes.

Linux native helper compilation requires `libx11-dev`, `libxi-dev`, a C/C++
toolchain and the normal Electron runtime libraries. `npm run dist` produces an
AppImage on Linux. Additional targets are available through `dist:linux` in the
desktop workspace.

macOS defaults to ZIP. `dist:mac:dmg` requires an independently prepared dmgbuild
tool tree. Signing and notarization use the existing packaging hooks and explicit
release credentials. Windows uses MSIX with this fork's identity and
`hermes-rune://` activation. Build and verify native artifacts on their target OS;
Linux verification does not establish macOS or Windows release readiness.

Updates are manual from the fork's releases. The original agent installer,
bundled-runtime release pipeline and in-app `hermes update` integration have
been removed. Protocol snapshots and their upstream revision are recorded in
`desktop-baseline.json`; builds do not regenerate them from backend Python.
