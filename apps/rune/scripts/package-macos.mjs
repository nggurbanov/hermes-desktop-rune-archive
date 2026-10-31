import { fileURLToPath } from 'node:url'
import { chmodSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { copy, directory, id, inputs, notices, options, product, root, run, xml } from './package-common.mjs'

export function appBundle(o) {
  const { binary, out, version } = inputs(o)
  const contents = directory(resolve(out, 'Contents'))
  copy(binary, resolve(contents, 'MacOS/HermesDesktopRune')); chmodSync(resolve(contents, 'MacOS/HermesDesktopRune'), 0o755)
  copy(resolve(root, 'apps/desktop/assets/icon.icns'), resolve(contents, 'Resources/icon.icns'))
  notices(directory(resolve(contents, 'Resources/Notices')))
  writeFileSync(resolve(contents, 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${id}</string><key>CFBundleName</key><string>${product}</string><key>CFBundleDisplayName</key><string>${product}</string><key>CFBundleExecutable</key><string>HermesDesktopRune</string><key>CFBundleIconFile</key><string>icon</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>${xml(version)}</string><key>CFBundleVersion</key><string>${xml(version)}</string><key>LSMinimumSystemVersion</key><string>13.0</string><key>LSApplicationCategoryType</key><string>public.app-category.productivity</string><key>NSMicrophoneUsageDescription</key><string>${product} uses the microphone for voice conversations.</string><key>NSAudioCaptureUsageDescription</key><string>${product} uses audio capture for voice conversations.</string><key>NSLocalNetworkUsageDescription</key><string>${product} connects to your Hermes agents on the local network.</string><key>CFBundleURLTypes</key><array><dict><key>CFBundleURLSchemes</key><array><string>hermes-rune</string></array></dict></array></dict></plist>\n`)
  return out
}
export function main(o) {
  if (process.platform !== 'darwin') throw new Error('macOS packaging requires macOS tools')
  const app = appBundle(o)
  if (o.identity) {
    run('codesign', ['--force', '--options', 'runtime', '--timestamp', '--entitlements', resolve(root, 'apps/rune/packaging/macos-entitlements.plist'), '--sign', o.identity, app])
    run('codesign', ['--verify', '--strict', '--verbose=2', app])
  } else if (o.notaryProfile) throw new Error('Notarization requires --identity')
  else console.error('Unsigned development bundle: no --identity supplied')
  if (o.notaryProfile) {
    const upload = `${app}.notary.zip`
    run('ditto', ['-c', '-k', '--keepParent', app, upload])
    run('xcrun', ['notarytool', 'submit', upload, '--keychain-profile', o.notaryProfile, '--wait'])
    run('xcrun', ['stapler', 'staple', app]); run('xcrun', ['stapler', 'validate', app])
  }
  if (o.zip) run('ditto', ['-c', '-k', '--keepParent', app, resolve(o.zip)])
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(options(process.argv.slice(2)))
