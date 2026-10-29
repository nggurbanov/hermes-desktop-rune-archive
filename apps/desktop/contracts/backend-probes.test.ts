import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { test } from 'vitest'

import { contractRuntime } from './runtime'

import { canImportHermesCli } from '../electron/backend-probes'

const { root: REPO, python: PYTHON } = contractRuntime()

interface RuntimeFixture {
  root: string
  python: string
  site: string
  dependencies: string
}

test('the real bootstrap supplies ruamel-only dependencies and rejects foreign-path rescue', async (): Promise<void> => {
  const temp: string = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-probe-runtime-'))
  const home: string = path.join(temp, 'home')

  const env: NodeJS.ProcessEnv = {
    ...process.env,
    HOME: home,
    HERMES_HOME: home,
    HERMES_RUNTIME_DIR: path.join(temp, 'tools'),
    HERMES_DISABLE_LAZY_INSTALLS: '1',
    UV_CACHE_DIR: path.join(temp, 'cache'),
    UV_NO_CONFIG: '1',
    PYTHONPATH: '',
    PYTHONHOME: '',
    PYTHONDONTWRITEBYTECODE: '1',
    USERPROFILE: home
  }

  const setup: string = `
import json, os, re, shutil, subprocess, sys, tomllib, venv
from pathlib import Path
repository, temp = map(Path, sys.argv[1:])
root = temp / 'external-runtime-copy'
root.mkdir()
for name in ('hermes_cli', 'hermes_platform', 'pm', 'agent', 'tools', 'gateway', 'tui_gateway', 'cron', 'plugins'):
    shutil.copytree(repository / name, root / name,
                    ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
for source in repository.glob('*.py'):
    shutil.copy2(source, root / source.name)
shutil.copy2(repository / 'pyproject.toml', root / 'pyproject.toml')
sys.path.insert(0, str(root))
from pm.environments import install_state_dir, runtime_facts_path, site_packages
seed = temp / 'seed'
venv.EnvBuilder(with_pip=False).create(seed)
dependencies = temp / 'dependencies'
manifest = tomllib.loads((root / 'pyproject.toml').read_text(encoding='utf-8'))
specs = [spec for spec in manifest['project']['dependencies']
         if re.split(r'[<>=;\\[]', spec, maxsplit=1)[0].lower() in ('ruamel.yaml', 'python-dotenv')]
assert len(specs) == 2, specs
subprocess.run([shutil.which('uv'), 'pip', 'install', '--python', sys.executable,
                '--target', str(dependencies), '--no-deps', *specs],
               check=True, stdout=sys.stderr, timeout=60)
selected = install_state_dir(root) / 'environments' / 'selected' / 'venv'
site = site_packages(selected)
site.mkdir(parents=True)
(selected / 'pyvenv.cfg').write_text('home = fixture\\n', encoding='utf-8')
(site / 'selected-dependencies.pth').write_text(str(dependencies) + '\\n', encoding='utf-8')
from pm.lock import Facts
Facts(runtime_facts_path(root)).record_state('venv', 'fixture', [], environment=selected)
python = seed / ('Scripts/python.exe' if os.name == 'nt' else 'bin/python')
print(json.dumps({'root': str(root), 'python': str(python), 'site': str(site), 'dependencies': str(dependencies)}))
`

  try {
    const fixture: RuntimeFixture = JSON.parse(
      execFileSync(PYTHON, ['-I', '-c', setup, REPO, temp], {
        cwd: temp,
        env,
        encoding: 'utf8',
        timeout: 90_000,
        windowsHide: true
      })
    ) as RuntimeFixture

    assert.equal(await canImportHermesCli(fixture.python, { cwd: fixture.root, env }), true)
    assert.equal(
      await canImportHermesCli(fixture.python, {
        cwd: fixture.root,
        env: { ...env, PYTHONHOME: path.join(temp, 'foreign-home') }
      }),
      true,
      'Python home overrides must be scrubbed before the interpreter starts'
    )

    fs.unlinkSync(path.join(fixture.site, 'selected-dependencies.pth'))
    const foreign: string = path.join(temp, 'foreign-packages')
    fs.symlinkSync(fixture.dependencies, foreign, process.platform === 'win32' ? 'junction' : 'dir')
    assert.equal(
      await canImportHermesCli(fixture.python, {
        cwd: fixture.root,
        env: { ...env, PYTHONPATH: foreign }
      }),
      false,
      'foreign dependencies must not conceal an empty selected environment'
    )
    assert.equal(
      await canImportHermesCli(fixture.python, {
        cwd: fixture.root,
        env: { ...env, PYTHONHOME: path.join(temp, 'foreign-home') }
      }),
      false
    )
  } finally {
    fs.rmSync(temp, { recursive: true, force: true, maxRetries: 3 })
  }
}, 120_000)
