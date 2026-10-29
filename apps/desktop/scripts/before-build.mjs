import fs from 'node:fs'
import path from 'node:path'
import productIdentity from '../product-identity.cjs'

/** Stage the desktop's Windows tile artwork; no agent launchers are packaged. */
export function stageMsixAssets(desktop = path.resolve(import.meta.dirname, '..')) {
  const sourceDir = path.join(desktop, 'assets', 'appx')
  const stageDir = path.join(desktop, 'build', 'appx')
  const names = fs.readdirSync(sourceDir).filter(name => name.endsWith('.png'))
  for (const required of ['Square44x44Logo.png', 'Square150x150Logo.png', 'StoreLogo.png', 'Wide310x150Logo.png']) {
    if (!names.includes(required)) throw new Error(`missing MSIX asset ${path.join(sourceDir, required)}`)
  }
  fs.mkdirSync(stageDir, { recursive: true })
  for (const name of names) fs.copyFileSync(path.join(sourceDir, name), path.join(stageDir, name))
}

export default async function beforeBuild() {
  stageMsixAssets()
  writeMsixExtensions()
  return false
}

/** The hardware key launches this client without exposing agent CLI aliases. */
export function writeMsixExtensions(desktop = path.resolve(import.meta.dirname, '..'), identity = productIdentity) {
  const escape = value => String(value).replace(/[&<>"']/g, character => `&#${character.charCodeAt(0)};`)
  const displayName = escape(identity.displayName)
  const protocol = escape(identity.protocolScheme)
  const output = path.join(desktop, 'build', 'msix-extensions.xml')
  // Copilot key properties use unprefixed child names in Microsoft's schema.
  const xml = `<uap3:Extension
    xmlns:uap3="http://schemas.microsoft.com/appx/manifest/uap/windows10/3"
    Category="windows.appExtension">
  <uap3:AppExtension
      Name="com.microsoft.windows.copilotkeyprovider"
      Id="CopilotKeyProvider"
      DisplayName="${displayName}"
      Description="Launch ${displayName} with the Copilot key"
      PublicFolder="Public">
    <uap3:Properties>
      <SingleTap>${protocol}://copilot-key/start?state=Tap</SingleTap>
      <PressAndHoldStart>${protocol}://copilot-key/start?state=Down</PressAndHoldStart>
      <PressAndHoldStop>${protocol}://copilot-key/stop?state=Up</PressAndHoldStop>
    </uap3:Properties>
  </uap3:AppExtension>
</uap3:Extension>
`
  fs.mkdirSync(path.dirname(output), { recursive: true })
  fs.writeFileSync(output, xml)
  return output
}
