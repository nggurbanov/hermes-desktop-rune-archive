import { fileURLToPath } from 'node:url'
import { chmodSync, symlinkSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { copy, directory, id, inputs, notices, options, product, required, root, run } from './package-common.mjs'

export function appDir(o) {
  const { binary, out, version } = inputs(o)
  directory(out)
  copy(binary, resolve(out, 'usr/bin/hermes-desktop-rune'))
  chmodSync(resolve(out, 'usr/bin/hermes-desktop-rune'), 0o755)
  copy(resolve(o.sourceRoot || root, 'apps/desktop/assets/icon.png'), resolve(out, `${id}.png`))
  writeFileSync(resolve(out, `${id}.desktop`), `[Desktop Entry]\nType=Application\nName=${product}\nExec=hermes-desktop-rune %u\nIcon=${id}\nCategories=Utility;\nMimeType=x-scheme-handler/hermes-rune;\n`)
  writeFileSync(resolve(out, 'AppRun'), '#!/bin/sh\nHERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"\nexec "$HERE/usr/bin/hermes-desktop-rune" "$@"\n', { mode: 0o755 })
  symlinkSync(`${id}.png`, resolve(out, '.DirIcon'))
  notices(directory(resolve(out, 'usr/share/doc/hermes-desktop-rune')), o.sourceRoot || root)
  writeFileSync(resolve(out, 'usr/share/doc/hermes-desktop-rune/VERSION'), `${version}\n`)
  return out
}
export function main(o) {
  const runtime = o.appimage ? resolve(required(o, 'runtime')) : null
  const dir = appDir(o)
  if (o.appimage) run(o.appimagetool || 'appimagetool', ['--runtime-file', runtime, dir, resolve(o.appimage)])
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(options(process.argv.slice(2)))
