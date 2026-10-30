# Passive host inventory provenance

This native client adapter follows the retained Desktop baseline identified by
`desktop-baseline.json` (`ef75bc708867e3884cb40a91363be6dc6fddc034`). It does not
bundle or import the Hermes agent runtime.

Policy sources retained in this repository:

- `apps/desktop/electron/main.ts::probeSshProfileInventory`: inventory an SSH
  source without ensuring a backend, opening a forward or starting a dashboard.
- `electron/remote-lifecycle.ts::probeRemoteHermesHome`,
  `listRemoteHermesProfiles`, `readRemoteInstallId`, `remoteInstallRoot`: resolve
  the remote environment's home, inspect existing profile directories and read
  the existing install-root identity without minting one.
- `electron/connection-registry.ts::parseRemoteProfileListing`,
  `shouldRetrySshInventory`, `rememberSshEnumeration`: validated names, retained
  successful inventory and a 60-second failure retry policy.
- `electron/windows-remote-inventory.ts` and `windows-runtime-command.ts`: the
  baseline Windows inventory uses an independently installed runtime launcher.

`ssh.rs` is an original Rust/OpenSSH adaptation of these client policies.
The local process owner is implemented for Linux and macOS; only Linux has
executed native process-tree proof at this checkpoint.
The static program sent on stdin supports a POSIX Linux/macOS remote. It reads
only the current environment, directory names and `install_id`; it creates no
home, identity, forward, control master, agent owner or background service. A
missing home is an error rather than evidence for a default agent. Names are
validated before being encoded, as well as when parsed, so a directory containing
newlines cannot inject additional profile rows.

Inventory proves filesystem names at the authenticated SSH target. It does not
prove that a gateway is running or serves those names; its public evidence is
`SshInventory` and availability is `ConnectOnDemand`. Local roster enumeration
uses the existing private native control socket and HTTP tickets instead of a
filesystem fallback. A deferred Local source retains no invented profile list.

The current saved SSH shape has a gateway URL path prefix, not a runtime launcher
path. Windows remote detection/launcher inventory is not implemented here. An
unsupported or insufficiently configured remote reports inventory unavailability;
no POSIX failure is converted into a successful profile list. Full Windows SSH
inventory parity remains an explicit requirement.

Every locally spawned inventory child is registered in a dedicated process
group before it can race shutdown. Nonblocking stdin/stdout/stderr share the
request deadline/cancellation loop; inherited pipes cannot hold a joining reader
thread. The leader is observed with WNOWAIT, then its group is killed before the
leader is reaped, so its PID cannot be reused to address an unrelated group.
The last UI subscriber performs the same owned cleanup, even when a cache-only
handle survives. Process registration keys are monotonic and distinct from OS
PIDs. Private Linux subreaper/pidfd contracts prove successful leader exit,
cancellation, deadline and actual helper cleanup; macOS execution remains open.

Windows local ownership needs a Job-object implementation before this passive
SSH path may spawn there. It currently reports unavailability before spawning;
this is an explicit remaining implementation gap, not Windows parity. This local
host gap is separate from Windows **remote** runtime inventory described above.
