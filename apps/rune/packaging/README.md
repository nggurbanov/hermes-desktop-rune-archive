# Native Rune packaging

These recipes accept an already linked native release executable and do not ship an agent, Node, Python or an updater. Run on the target host with Node. Output trees must not exist. Version is three or four numeric components (MSIX adds `.0` to three); the Rust prerelease version must be mapped deliberately by release engineering.

```
node apps/rune/scripts/package-linux.mjs --binary /path/hermes-rune --out /tmp/Hermes.AppDir --version 1.2.3 --appimage /tmp/Hermes.AppImage --appimagetool /path/appimagetool --runtime /path/pinned-type2-runtime
node apps/rune/scripts/package-macos.mjs --binary /path/hermes-rune --out /tmp/Hermes.app --version 1.2.3 --zip /tmp/Hermes.zip --identity 'Developer ID Application: …' --notaryProfile KEYCHAIN_PROFILE
node apps/rune/scripts/package-windows.mjs --binary C:/path/hermes-rune.exe --out C:/tmp/HermesTree --version 1.2.3 --arch x64 --publisher 'CN=Publisher' --makeappx C:/path/makeappx.exe --msix C:/tmp/Hermes.msix --signtool C:/path/signtool.exe --thumbprint CERTIFICATE_THUMBPRINT --timestamp https://timestamp.example
```

For Linux omit `--appimage` to assemble an AppDir alone. appimagetool and a separately pinned type2 runtime are explicit external prerequisites; this recipe does not bundle GTK/WebKitGTK, graphics or other host libraries. Establish and document compatible host versions and AppImage runtime architecture before distribution. For macOS omit identity/profile only for an explicitly unsigned development build; signing uses hardened runtime and microphone entitlement, not Electron JIT entitlements. A notary profile must already exist in the host keychain. For Windows omit thumbprint only for an unsigned development package; install requires a trusted signature. Import a certificate separately, never put secrets in arguments or source. The signed certificate subject must match Publisher. `makeappx` and the Windows SDK `signtool` must be supplied explicitly.

The GitHub matrix compiles development, test targets and release, and executes two nonhardware composer behavior tests on real hosted OSes; it publishes an unsigned binary, locked source archive, notices, hashes and provenance, not an installable release or proof of a clean install. It deliberately excludes execution of device, native-window and platform acceptance tests. Release engineers must independently package, sign, notarize, inspect linkage and validate installed child modes, URL/notification activation, WebView2/VC runtime and clean-host install/upgrade/uninstall. Verify the packaged binary uses the matching desktop-entry identity before claiming notification activation; older frozen candidates may still send `hermes`. Protocol manifests declare registration but do not establish app-side URL handling. The agent's update API is not a desktop updater.

## Acceptance and identity handoff

The installed desktop entry is `io.github.nggurbanov.hermes-desktop-rune.desktop`. The notification transport sends `desktop-entry` as `io.github.nggurbanov.hermes-desktop-rune` (without `.desktop`), matching the packaged desktop ID. Verify notification activation on a real desktop before release.

`package-linux-check.mjs` accepts installed AppRun and executable paths and exercises the read-only connection check against a private loopback gateway. It does not exercise native child windows or desktop integration. Release validation must use a binary and bundled modified sources from the same recorded build snapshot, with a complete corresponding source archive alongside the release; copying an older executable into a newer checkout alone does not establish this relationship.

## Frozen Linux checkpoint provenance

For an immutable candidate, use `package-provenance.mjs` instead of assembling from the live checkout. Supply the independently recorded SHA-256 of the frozen manifest, its exact binary, source tar and NUL-delimited source list, and a new output path for the corresponding source archive:

```
node apps/rune/scripts/package-provenance.mjs --manifest /path/source-manifest.json --manifest-sha256 SHA256 --binary /path/hermes-rune --source-archive /path/source.tar --source-list /path/source-files.list0 --source-output /path/output-source.tar --out /path/Hermes.AppDir --version 1.2.3 --appimage /path/Hermes.AppImage --appimagetool /path/appimagetool --appimagetool-sha256 SHA256 --runtime /path/type2-runtime --runtime-sha256 SHA256
```

The script stages and hashes each input, extracts the checked archive to obtain the icon, licenses, notices and modified dependency sources, checks the original inputs again, and records hashes in `usr/share/doc/hermes-desktop-rune/PROVENANCE.json`. The manifest is included there; retain the output source archive with the artifact. These hashes bind the packaged files to the supplied build record; they do not independently prove that the executable was built from that source or authenticate the record. Verify the manifest digest through a trusted channel. Failed runs may leave incomplete output; do not distribute it. The AppImage needs compatible host GTK3, WebKitGTK 4.1, ALSA, D-Bus and graphics libraries. This checkpoint does not establish clean-host installation, native child/notification activation, or macOS/Windows behavior.
