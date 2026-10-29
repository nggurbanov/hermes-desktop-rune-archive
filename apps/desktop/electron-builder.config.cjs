// The desktop package contains the client and its native helpers only.
// @ts-check
'use strict'

const fs = require('node:fs')
const path = require('node:path')
const identity = require('./product-identity.cjs')
const { createMacSigner } = require('./scripts/mac-sign.mjs')
const { macIconResource } = require('./scripts/mac-icon.cjs')
const electronVersion = require('./package.json').devDependencies.electron

/** @satisfies {import('app-builder-lib').Configuration} */
module.exports = {
  electronVersion,
  appId: identity.appId,
  productName: identity.displayName,
  executableName: identity.windowsExecutableName,
  artifactName: `${identity.artifactNamePascal}-\${version}-\${os}-\${arch}.\${ext}`,
  icon: 'assets/icon',
  protocols: [{ name: `${identity.displayName} Protocol`, schemes: [identity.protocolScheme] }],
  publish: [{ provider: 'github', owner: 'nggurbanov', repo: 'hermes-desktop-rune' }],
  extraMetadata: {
    name: identity.cliName,
    productName: identity.displayName,
    desktopName: identity.appId
  },
  directories: { output: 'release' },
  files: ['dist/**', 'assets/**', 'public/**', 'package.json'],
  beforeBuild: 'scripts/before-build.mjs',
  beforePack: 'scripts/before-pack.mjs',
  afterExtract: 'scripts/after-extract.mjs',
  afterPack: 'scripts/after-pack.mjs',
  ...(process.platform === 'darwin' ? { afterSign: 'scripts/notarize.mjs' } : {}),
  extraResources: [{ from: 'build/install-stamp.json', to: 'install-stamp.json' }],
  asar: { unpack: ['**/*.node', '**/prebuilds/**', 'dist/**'] },
  mac: {
    icon: macIconResource(__dirname),
    notarize: false,
    minimumSystemVersion: '13.0',
    category: 'public.app-category.productivity',
    // A ZIP release needs no Python DMG toolchain.
    target: ['zip'],
    extendInfo: {
      CFBundleDisplayName: identity.displayName,
      NSAudioCaptureUsageDescription: `${identity.displayName} uses audio capture for voice conversations.`,
      NSCameraUsageDescription: `${identity.displayName} uses the camera when a feature you enable requests it.`,
      NSMicrophoneUsageDescription: `${identity.displayName} uses the microphone for voice conversations.`,
      NSCalendarsUsageDescription: `${identity.displayName} accesses Calendar when you request it.`,
      NSCalendarsFullAccessUsageDescription: `${identity.displayName} reads and manages Calendar when you request it.`,
      NSRemindersUsageDescription: `${identity.displayName} accesses Reminders when you request it.`,
      NSRemindersFullAccessUsageDescription: `${identity.displayName} reads and manages Reminders when you request it.`,
      NSAppleMusicUsageDescription: `${identity.displayName} accesses your music library when you request it.`,
      NSContactsUsageDescription: `${identity.displayName} accesses Contacts when you request it.`,
      NSScreenCaptureUsageDescription: `${identity.displayName} captures the screen when you ask it to.`,
      NSLocalNetworkUsageDescription: `${identity.displayName} connects to your Hermes agents on the local network.`,
      NSAppleEventsUsageDescription: `${identity.displayName} automates apps when you explicitly ask it to.`
    },
    sign: createMacSigner({
      entitlements: path.join(__dirname, 'electron/entitlements.mac.plist'),
      entitlementsInherit: path.join(__dirname, 'electron/entitlements.mac.inherit.plist'),
      hardenedRuntime: true,
      ignore: file => fs.lstatSync(file).isDirectory() || !isMachO(file)
    })
  },
  dmg: {
    shrink: false,
    icon: 'packaging/dmg-volume.icns',
    title: `${identity.displayName} Installer`,
    background: 'packaging/nous-dmg-2b.tiff',
    iconSize: 96,
    iconTextSize: 11,
    window: { width: 660, height: 400 },
    contents: [{ x: 253, y: 238, type: 'file' }, { x: 512, y: 235, type: 'link', path: '/Applications' }]
  },
  win: {
    executableName: identity.windowsExecutableName,
    legalTrademarks: identity.displayName,
    target: ['msix'],
    extraResources: [{ from: 'scripts/update-relaunch-waiter.ps1', to: 'update-relaunch-waiter.ps1' }]
  },
  msix: {
    identityName: identity.msixAppIdWithOrg,
    applicationId: identity.appNamePascal,
    displayName: identity.displayName,
    publisher: 'CN=nggurbanov',
    publisherDisplayName: 'nggurbanov',
    customExtensionsPath: 'build/msix-extensions.xml',
    minVersion: '10.0.22621.0',
    maxVersionTested: '10.0.26100.0',
    showNameOnTiles: true
  },
  linux: {
    category: 'Utility',
    maintainer: 'nggurbanov',
    synopsis: 'Desktop client for local and remote Hermes agents.',
    target: ['AppImage']
  }
}

const MACHO_MAGICS = new Set([0xfeedface, 0xcefaedfe, 0xfeedfacf, 0xcffaedfe, 0xcafebabe, 0xbebafeca])

/** @param {string} file */
function isMachO(file) {
  const buffer = Buffer.alloc(4)
  const fd = fs.openSync(file, 'r')
  try {
    return fs.readSync(fd, buffer, 0, 4, 0) === 4 && MACHO_MAGICS.has(buffer.readUInt32BE(0))
  } finally {
    fs.closeSync(fd)
  }
}
