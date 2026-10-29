// This client owns its OS identity independently of every Hermes installation.
// @ts-check
/// <reference types="node" />
'use strict'

const variant = process.env.HERMES_DESKTOP_VARIANT || 'standalone'
if (variant !== 'standalone') {
  throw new Error(`This desktop packages only the standalone client, not ${variant}`)
}

/** @type {import('./product-identity.d.cts')} */
const identity = {
  light: false,
  store: false,
  displayName: 'Hermes Desktop Rune',
  appId: 'io.github.nggurbanov.hermes-desktop-rune',
  appNamePascal: 'HermesDesktopRune',
  artifactNamePascal: 'HermesDesktopRune',
  windowsExecutableName: 'HermesDesktopRune',
  cliName: 'hermes-desktop-rune',
  msixAppIdWithOrg: 'nggurbanov.HermesDesktopRune',
  protocolScheme: 'hermes-rune',
  channel: 'latest'
}

module.exports = Object.freeze(identity)
