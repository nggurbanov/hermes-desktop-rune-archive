import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
const dir = process.argv[2]
if (!dir) throw new Error('Usage: node package-hashes.mjs directory')
const entries = readdirSync(dir, {withFileTypes:true}).filter(e => e.isFile() && e.name !== 'SHA256SUMS').sort((a,b) => a.name.localeCompare(b.name))
writeFileSync(join(dir, 'SHA256SUMS'), entries.map(e => `${createHash('sha256').update(readFileSync(join(dir, e.name))).digest('hex')}  ${e.name}\n`).join(''))
