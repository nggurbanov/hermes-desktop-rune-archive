interface ProductIdentity {
  /** Legacy shape retained for client consumers; standalone supports local and remote. */
  light: boolean
  /** True for a Store-submission build (Windows Store packaging identity). */
  store: boolean
  /** Desktop display name. */
  displayName: string
  /** Independent OS-level app identity. */
  appId: string
  /** App name in Pascal case. */
  appNamePascal: string
  /** Artifact prefix stays compatible with release archive consumers. */
  artifactNamePascal: string
  /** Windows GUI executable stem; not the payload CLI launcher path. */
  windowsExecutableName: string
  /** Desktop executable name, never the agent CLI command. */
  cliName: string
  /** App-specific deep links never replace the official hermes:// handler. */
  protocolScheme: string
  /** Windows package identity. */
  msixAppIdWithOrg: string
  /** R2 identity token on channel builds; absent on legacy products. */
  readonly token?: string
  /** Channel subscription for channel builds; legacy electron-updater label otherwise. Stable tags:
   *  "latest" | "light"; canary tags: "canary" | "light-canary". Null
   *  for Store and commit builds (no release feed). */
  readonly channel: string | null
  /** Store-submission MSIX packaging identity. Present only when `store`. */
  storeMsix?: {
    identityName: string
    publisher: string
    publisherDisplayName: string
  }
}

declare const identity: ProductIdentity
export = identity
