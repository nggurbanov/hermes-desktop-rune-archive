import { fileURLToPath } from 'node:url'
import { writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { copy, directory, inputs, notices, options, product, required, root, run, xml } from './package-common.mjs'

export function packageTree(o) {
  const { binary, out, version } = inputs(o)
  const publisher = required(o, 'publisher')
  if (!/^CN=.+/.test(publisher) || /[\r\n]/.test(publisher)) throw new Error('Publisher must be an X.500 distinguished name beginning CN=')
  const arch = required(o, 'arch')
  if (!['x64', 'arm64', 'x86'].includes(arch)) throw new Error('Invalid MSIX architecture')
  const msixVersion = version.split('.').length === 3 ? `${version}.0` : version
  directory(out); copy(binary, resolve(out, 'HermesDesktopRune.exe'))
  notices(directory(resolve(out, 'Notices')))
  directory(resolve(out, 'Assets'))
  writeFileSync(resolve(out, 'AppxManifest.xml'), `<?xml version="1.0" encoding="utf-8"?>\n<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10" xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities" IgnorableNamespaces="uap desktop rescap"><Identity Name="nggurbanov.HermesDesktopRune" Publisher="${xml(publisher)}" Version="${msixVersion}" ProcessorArchitecture="${arch}"/><Properties><DisplayName>${product}</DisplayName><PublisherDisplayName>Hermes</PublisherDisplayName><Logo>Assets\\StoreLogo.png</Logo></Properties><Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0"/></Dependencies><Resources><Resource Language="en-us"/></Resources><Applications><Application Id="HermesDesktopRune" Executable="HermesDesktopRune.exe" EntryPoint="Windows.FullTrustApplication"><uap:VisualElements DisplayName="${product}" Description="${product}" BackgroundColor="transparent" Square150x150Logo="Assets\\Square150x150Logo.png" Square44x44Logo="Assets\\Square44x44Logo.png"><uap:DefaultTile Wide310x150Logo="Assets\\Wide310x150Logo.png"/></uap:VisualElements><Extensions><uap:Extension Category="windows.protocol"><uap:Protocol Name="hermes-rune"/></uap:Extension></Extensions></Application></Applications><Capabilities><rescap:Capability Name="runFullTrust"/></Capabilities></Package>\n`)
  return out
}
export function main(o) {
  if (process.platform !== 'win32') throw new Error('MSIX packaging requires Windows SDK tools on Windows')
  const makeappx = required(o, 'makeappx'); const packagePath = resolve(required(o, 'msix'))
  const dir = packageTree(o)
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-File', resolve(root, 'apps/rune/packaging/windows-icons.ps1'), resolve(root, 'apps/desktop/assets/icon.png'), resolve(dir, 'Assets')])
  run(makeappx, ['pack', '/d', dir, '/p', packagePath])
  if (o.thumbprint) {
    const signtool = required(o, 'signtool')
    run(signtool, ['sign', '/sha1', o.thumbprint, '/fd', 'SHA256', '/tr', required(o, 'timestamp'), '/td', 'SHA256', packagePath])
    run(signtool, ['verify', '/pa', '/v', packagePath])
    run('powershell.exe', ['-NoProfile', '-NonInteractive', '-File', resolve(root, 'apps/rune/packaging/windows-verify-publisher.ps1'), packagePath, o.publisher])
  } else console.error('Unsigned development MSIX: no --thumbprint supplied; Windows installation requires a trusted signature')
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(options(process.argv.slice(2)))
