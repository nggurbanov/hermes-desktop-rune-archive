/**
 * Write provenance for the standalone desktop client.
 * bundle-electron-main.mjs bakes it into the running code. The packaged
 * sidecar exists only to detect replacement of that artifact by an update.
 * Provenance comes from CI, local git, or an explicit unknown-source stamp.
 */

import { mkdirSync, readFileSync, writeFileSync } from "fs"
import { resolve, join, relative } from "path"
import { validateBundleEnvironment } from "./bundle-env.mjs"
import { execFileSync } from "child_process"

import { isMain } from "./utils.mjs"

const STAMP_SCHEMA_VERSION = 1

/** All-zero placeholder used when no real commit can be resolved. */
export const FALLBACK_COMMIT = "0000000000000000000000000000000000000000"
export const FALLBACK_BRANCH = "main"

const DESKTOP_ROOT = resolve(import.meta.dirname, "..")
const REPO_ROOT = resolve(DESKTOP_ROOT, "..", "..")
const OUT_DIR = join(DESKTOP_ROOT, "build")
const OUT_FILE = join(OUT_DIR, "install-stamp.json")

function tryExec(argv, opts) {
  try {
    return execFileSync(argv[0], argv.slice(1), { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 5000, ...opts }).trim()
  } catch {
    return null
  }
}

export function fromCI(env = process.env) {
  const sha = env.GITHUB_SHA
  if (!sha) return null
  const branch = env.GITHUB_REF_NAME || env.GITHUB_HEAD_REF || null
  return {
    commit: sha,
    branch: branch,
    dirty: false, // CI builds from a checkout-of-ref by definition
    source: "ci"
  }
}

export function fromLocalGit(repoRoot = REPO_ROOT, execFn = tryExec) {
  const sha = execFn(["git", "rev-parse", "HEAD"], { cwd: repoRoot })
  if (!sha) return null
  const branch = execFn(["git", "rev-parse", "--abbrev-ref", "HEAD"], { cwd: repoRoot })
  // `git status --porcelain -uno` is empty iff tracked files match HEAD.
  // We exclude untracked files (-uno) intentionally: a developer who's
  // checked out an installer scratch dir alongside the repo shouldn't
  // poison every local build with a [DIRTY] stamp.  We DO care about
  // tracked-but-modified files because those mean the .exe content
  // differs from the commit being pinned.
  const status = execFn(["git", "status", "--porcelain", "-uno"], { cwd: repoRoot })
  const dirty = status !== null && status.length > 0
  return {
    commit: sha,
    branch: branch === "HEAD" ? null : branch, // detached HEAD -> null
    dirty: dirty,
    source: "local"
  }
}

export function fromFallback(branch = FALLBACK_BRANCH) {
  // Source archives can still be built; the placeholder explicitly records
  // that their commit provenance is unknown.
  return {
    commit: FALLBACK_COMMIT,
    branch: branch || FALLBACK_BRANCH,
    dirty: false,
    source: "fallback"
  }
}

/**
 * Resolve the install stamp without writing it.  Pure enough for unit tests:
 * inject env / execFn / repoRoot to simulate CI, local git, or no-git trees.
 */
export function resolveStamp({
  env = process.env,
  repoRoot = REPO_ROOT,
  execFn = tryExec,
  fallbackBranch = FALLBACK_BRANCH
} = {}) {
  if (env.HERMES_BUILD_COMMIT) {
    if (!/^[a-f0-9]{40}$/.test(env.HERMES_BUILD_COMMIT) || env.HERMES_PAYLOAD_TAG) {
      throw new Error('Commit builds require an exact full SHA without a release tag')
    }
    const local = fromLocalGit(repoRoot, execFn)
    if (!local || local.commit !== env.HERMES_BUILD_COMMIT) {
      throw new Error('Commit build identity does not match the checkout HEAD')
    }
    return { ...local, branch: null, source: 'commit-build' }
  }
  return fromCI(env) || fromLocalGit(repoRoot, execFn) || fromFallback(fallbackBranch)
}

export function isFallbackCommit(commit) {
  return typeof commit === "string" && /^0{7,40}$/.test(commit)
}

/** The sidecar identifies only this desktop artifact. */
export function writeDesktopStamp(outDir, built) {
  const json = JSON.stringify(built, null, 2) + "\n"
  mkdirSync(outDir, { recursive: true })
  writeFileSync(join(outDir, 'install-stamp.json'), json, 'utf8')
}

function main() {
  const stamp = resolveStamp()
  if (!stamp || !stamp.commit) {
    // Should not happen — fromFallback() always provides a commit.
    console.error(
      "[write-build-stamp] ERROR: could not determine git commit.\n" +
        "  - $GITHUB_SHA not set\n" +
        "  - `git rev-parse HEAD` failed at " +
        REPO_ROOT +
        "\n" +
        "Packaged builds record this desktop checkout provenance.\n" +
        "Run from a git checkout or set $GITHUB_SHA explicitly."
    )
    process.exit(1)
  }

  if (isFallbackCommit(stamp.commit)) {
    console.warn(
      "[write-build-stamp] WARNING: no git commit found (non-git checkout?).\n" +
        "  Using placeholder commit in desktop provenance.\n" +
        "  For production builds,\n" +
        "  run from a git checkout or set $GITHUB_SHA."
    )
  }

  if (stamp.dirty) {
    console.warn(
      "[write-build-stamp] WARNING: working tree is dirty.\n" +
        "  Pinning to " +
        stamp.commit.slice(0, 12) +
        " but the packaged code may differ from that commit.\n" +
        "  Commit your changes before publishing this build."
    )
  }

  const built = buildStampPayload(stamp)
  writeDesktopStamp(OUT_DIR, built)
  console.log(
    "[write-build-stamp] wrote " +
      relative(REPO_ROOT, OUT_FILE) +
      " -> " +
      stamp.commit.slice(0, 12) +
      (stamp.branch ? " (" + stamp.branch + ")" : "") +
      (stamp.dirty ? " [DIRTY]" : "") +
      (stamp.source === "fallback" ? " [FALLBACK]" : "")
  )
}

/** The app version and update owner never come from a connected agent. */
export function buildStampPayload(stamp, env = process.env, _platform = process.platform) {
  const variant = (env.HERMES_DESKTOP_VARIANT || 'standalone').trim()
  if (variant !== 'standalone') throw new Error(`This desktop packages only the standalone client, not ${variant}`)
  if (env._HERMES_CHANNEL_REQUEST_JSON) throw new Error('Agent channel builds cannot package the standalone desktop')
  const commitBuild = env.HERMES_BUILD_COMMIT || null
  if (commitBuild && (!/^[a-f0-9]{40}$/.test(commitBuild) || commitBuild !== stamp.commit)) {
    throw new Error('Commit build identity does not match the stamp commit')
  }
  const { version } = JSON.parse(readFileSync(join(DESKTOP_ROOT, 'package.json'), 'utf8'))
  const bundleEnv = env.HERMES_BUNDLE_ENV_JSON ? validateBundleEnvironment(JSON.parse(env.HERMES_BUNDLE_ENV_JSON)) : undefined
  return {
    schemaVersion: STAMP_SCHEMA_VERSION,
    commit: stamp.commit,
    branch: commitBuild ? null : stamp.branch,
    builtAt: new Date().toISOString(),
    dirty: stamp.dirty,
    source: commitBuild ? 'commit-build' : stamp.source,
    commitDate: stamp.commitDate ?? null,
    baseVersion: version,
    displayVersion: version,
    distance: null,
    payload: 'standalone',
    distribution: 'desktop-app',
    // Installers are independently downloadable until an app-only updater is verified.
    updateMechanism: 'external',
    tag: null,
    ...(bundleEnv ? { bundleEnv } : {})
  }
}

if (isMain(import.meta.url)) {
  main()
}

