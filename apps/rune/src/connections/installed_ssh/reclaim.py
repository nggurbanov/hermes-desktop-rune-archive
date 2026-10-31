"""Reclaim a record only after proving its exact kernel incarnation is gone.

The caller holds the updater mutex shared with Electron and all native spawns.
A surviving owned session is recovered only through an exact nonce anchor and
pidfd-bound signals. An unprovable session is never signaled.
"""

import fcntl
import json
import os
import select
import signal
import stat
import sys
import time


def proc_fields(pid):
    with open("/proc/%d/stat" % pid) as stream:
        raw = stream.read()
    return raw[raw.rfind(")") + 2 :].split()


def group_members(group_id):
    members = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            fields = proc_fields(int(entry))
            if (
                fields[0] != "Z"
                and int(fields[2]) == group_id
                and int(fields[3]) == group_id
            ):
                members.append(int(entry))
        except FileNotFoundError:
            continue
        except (OSError, ValueError, IndexError):
            raise SystemExit(12)
    return members


def anchor_matches(pid, group_id, token, nonce, profile):
    try:
        if os.stat("/proc/%d" % pid).st_uid != os.getuid():
            return False
        fields = proc_fields(pid)
        if fields[0] == "Z" or int(fields[2]) != group_id or int(fields[3]) != group_id:
            return False
        with open("/proc/%d/cmdline" % pid, "rb") as stream:
            args = [
                os.fsdecode(part) for part in stream.read(65537).split(b"\0") if part
            ]
        serves = [
            i
            for i, arg in enumerate(args)
            if arg == "serve" and not (i > 0 and args[i - 1] == "--profile")
        ]
        if (
            len(serves) != 1
            or args.count("--isolated") != 1
            or args.index("--isolated") < serves[0]
        ):
            return False
        if (
            args.count("--ssh-owner-nonce") != 1
            or args[args.index("--ssh-owner-nonce") + 1] != nonce
        ):
            return False
        if (
            args.count("--ssh-session-token-file") != 1
            or args[args.index("--ssh-session-token-file") + 1] != token
        ):
            return False
        if profile:
            return (
                args.count("--profile") == 1
                and args[args.index("--profile") + 1] == profile
                and args.index("--profile") < serves[0]
            )
        return args.count("--profile") == 0
    except (OSError, ValueError, IndexError):
        return False


def recover_group(group_id, owner, nonce, profile):
    members = group_members(group_id)
    if not members:
        return
    token = os.path.join(owner, nonce + ".token")
    anchor = next(
        (
            pid
            for pid in members
            if anchor_matches(pid, group_id, token, nonce, profile)
        ),
        None,
    )
    if (
        anchor is None
        or not hasattr(os, "pidfd_open")
        or not hasattr(signal, "pidfd_send_signal")
    ):
        raise SystemExit(13)
    try:
        anchorfd = os.pidfd_open(anchor)
    except OSError:
        raise SystemExit(13)
    stopped = False
    try:
        # Freeze the exact nonce-bearing process. While it remains alive its
        # session/group identity cannot be replaced by another session.
        if not anchor_matches(anchor, group_id, token, nonce, profile):
            raise SystemExit(13)
        signal.pidfd_send_signal(anchorfd, signal.SIGSTOP)
        deadline = time.monotonic() + 2
        while True:
            fields = proc_fields(anchor)
            if (
                fields[0] in ("T", "t")
                and int(fields[2]) == group_id
                and int(fields[3]) == group_id
            ):
                stopped = True
                break
            if time.monotonic() >= deadline:
                raise SystemExit(13)
            time.sleep(0.02)
        anchor_poll = select.poll()
        anchor_poll.register(anchorfd, select.POLLIN)
        deadline = time.monotonic() + 5
        while True:
            others = [pid for pid in group_members(group_id) if pid != anchor]
            if not others:
                break
            if time.monotonic() >= deadline or anchor_poll.poll(0):
                raise SystemExit(13)
            for member in others:
                try:
                    memberfd = os.pidfd_open(member)
                except ProcessLookupError:
                    continue
                try:
                    # The frozen anchor binds session identity during this
                    # proof; pidfd binds each individual signal to that PID's
                    # incarnation even if it exits concurrently.
                    if anchor_poll.poll(0):
                        raise SystemExit(13)
                    fields = proc_fields(member)
                    if (
                        fields[0] != "Z"
                        and int(fields[2]) == group_id
                        and int(fields[3]) == group_id
                    ):
                        signal.pidfd_send_signal(memberfd, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                finally:
                    os.close(memberfd)
            time.sleep(0.02)
        signal.pidfd_send_signal(anchorfd, signal.SIGKILL)
        stopped = False
        if not anchor_poll.poll(2000) or group_members(group_id):
            raise SystemExit(13)
    finally:
        if stopped:
            try:
                signal.pidfd_send_signal(anchorfd, signal.SIGCONT)
            except ProcessLookupError:
                pass
        os.close(anchorfd)


owner_arg, expected_launcher, expected_home, expected_profile, expected_nonce = (
    sys.argv[1:]
)
owner = os.path.expanduser(owner_arg)
try:
    dirfd = os.open(
        owner,
        os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
    )
except FileNotFoundError:
    print("NONE")
    sys.exit(0)
try:
    info = os.fstat(dirfd)
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise SystemExit(2)
    lockfd = os.open(
        "reservation.lock",
        os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0),
        0o600,
        dir_fd=dirfd,
    )
    try:
        deadline = time.monotonic() + 10
        while True:
            try:
                fcntl.flock(lockfd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise SystemExit(3)
                time.sleep(0.02)
        try:
            fd = os.open(
                "backend.lock.json",
                os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
                dir_fd=dirfd,
            )
        except FileNotFoundError:
            print("NONE")
            sys.exit(0)
        with os.fdopen(fd, "rb") as stream:
            data = stream.read(65537)
        if len(data) > 65536:
            raise SystemExit(4)
        record = json.loads(data)
        nonce = record.get("spawnNonce")
        pid = record.get("pid")
        if (
            record.get("schemaVersion") != 2
            or record.get("protocolVersion") != 1
            or record.get("ownershipId") != os.path.basename(owner)
            or not isinstance(nonce, str)
            or nonce != expected_nonce
            or len(nonce) != 16
            or any(c not in "0123456789abcdef" for c in nonce)
            or not isinstance(pid, int)
            or isinstance(pid, bool)
            or pid <= 0
            or record.get("hermesPath") != expected_launcher
            or record.get("hermesHome") != expected_home
            or record.get("profile") != expected_profile
            or record.get("logPath") != owner_arg + "/" + nonce + ".log"
        ):
            raise SystemExit(5)
        if record.get("supervisorProtocol") != 1:
            # A legacy owner may be attachable, but this build cannot prove
            # its process-tree containment and must not reclaim it.
            print("FOREIGN")
            sys.exit(0)
        ticks = record.get("startTicks")
        if not isinstance(ticks, int) or isinstance(ticks, bool):
            creation = record.get("creationTime", "")
            ticks = (
                int(creation[6:])
                if isinstance(creation, str)
                and creation.startswith("linux:")
                and creation[6:].isdigit()
                else None
            )
        if not os.path.exists("/proc/self/stat"):
            raise SystemExit(6)
        try:
            with open("/proc/%d/stat" % pid) as stream:
                raw = stream.read()
            fields = raw[raw.rfind(")") + 2 :].split()
            actual = int(fields[19])
            # A live PID whose start ticks differ may be unrelated. Never
            # remove a record on that mismatch; its provenance is unproved.
            if fields[0] != "Z" and ticks is not None and actual != ticks:
                raise SystemExit(7)
            dead = fields[0] == "Z"
        except FileNotFoundError:
            dead = True
        except (OSError, ValueError, IndexError):
            raise SystemExit(6)
        if not dead:
            print("LIVE")
            sys.exit(0)
        recover_group(pid, owner, nonce, expected_profile)
        # Names are derived only from the validated generation nonce.  Never
        # follow paths from the record and never remove a foreign process.
        for name in ("backend.lock.json", nonce + ".log", nonce + ".token"):
            try:
                os.unlink(name, dir_fd=dirfd)
            except FileNotFoundError:
                pass
        print("RECLAIMED:" + nonce)
    finally:
        os.close(lockfd)
finally:
    os.close(dirfd)
