# Remove only a generation whose persisted identity and live argv both match.
import fcntl
import json
import os
import select
import signal
import stat
import sys
import time


def group_still_has_members(group_id):
    # The supervisor creates a new session whose session ID and process group
    # ID are its own PID. A live member means that exact group cannot yet be
    # declared gone; uncertainty is deliberately fail-closed.
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open("/proc/%s/stat" % entry) as stream:
                raw = stream.read()
            fields = raw[raw.rfind(")") + 2 :].split()
            if (
                fields[0] != "Z"
                and int(fields[2]) == group_id
                and int(fields[3]) == group_id
            ):
                return True
        except FileNotFoundError:
            continue
        except (OSError, ValueError, IndexError):
            raise SystemExit(12)
    return False


owner, nonce = sys.argv[1:]
owner = os.path.expanduser(owner)
if len(nonce) != 16 or any(c not in "0123456789abcdef" for c in nonce):
    raise SystemExit(2)
dirfd = os.open(
    owner, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
)
try:
    st = os.fstat(dirfd)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid != os.getuid():
        raise SystemExit(3)
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
                    raise SystemExit(10)
                time.sleep(0.02)
        try:
            fd = os.open(
                "backend.lock.json",
                os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
                dir_fd=dirfd,
            )
        except FileNotFoundError:
            try:
                os.unlink(nonce + ".token", dir_fd=dirfd)
            except FileNotFoundError:
                pass
            print("CLEAN")
            sys.exit(0)
        try:
            with os.fdopen(fd, "rb") as f:
                record = json.loads(f.read(65537))
        except BaseException:
            raise SystemExit(4)
        if record.get("schemaVersion") != 2 or record.get(
            "ownershipId"
        ) != os.path.basename(owner):
            raise SystemExit(5)
        if record.get("spawnNonce") != nonce:
            try:
                os.unlink(nonce + ".token", dir_fd=dirfd)
            except FileNotFoundError:
                pass
            print("FOREIGN")
            sys.exit(0)
        if record.get("supervisorProtocol") != 1:
            raise SystemExit(14)
        pid = record.get("pid")
        ticks = record.get("startTicks")
        profile = record.get("profile")
        if (
            not isinstance(pid, int)
            or pid <= 0
            or not isinstance(ticks, int)
            or not isinstance(profile, str)
        ):
            raise SystemExit(6)
        try:
            pidfd = os.pidfd_open(pid)
        except ProcessLookupError:
            pidfd = None
        if pidfd is None and group_still_has_members(pid):
            raise SystemExit(13)
        if pidfd is not None:
            try:
                try:
                    with open("/proc/%d/stat" % pid) as f:
                        raw = f.read()
                    actual = int(raw[raw.rfind(")") + 2 :].split()[19])
                    with open("/proc/%d/cmdline" % pid, "rb") as f:
                        args = [os.fsdecode(x) for x in f.read().split(b"\0") if x]
                except (OSError, ValueError, IndexError):
                    raise SystemExit(7)
                token = os.path.join(owner, nonce + ".token")
                try:
                    serve = [
                        i
                        for i, a in enumerate(args)
                        if a == "serve" and not (i > 0 and args[i - 1] == "--profile")
                    ]
                    ok = (
                        actual == ticks
                        and len(serve) == 1
                        and args.count("--isolated") == 1
                        and args.index("--isolated") > serve[0]
                        and args.count("--ssh-owner-nonce") == 1
                        and args[args.index("--ssh-owner-nonce") + 1] == nonce
                        and args.count("--ssh-session-token-file") == 1
                        and args[args.index("--ssh-session-token-file") + 1] == token
                        and (
                            (
                                args.count("--profile") == 1
                                and args[args.index("--profile") + 1] == profile
                                and args.index("--profile") < serve[0]
                            )
                            if profile
                            else args.count("--profile") == 0
                        )
                    )
                except (ValueError, IndexError):
                    ok = False
                if not ok:
                    raise SystemExit(8)
                signal.pidfd_send_signal(pidfd, signal.SIGTERM)
                poll = select.poll()
                poll.register(pidfd, select.POLLIN)
                if not poll.poll(5000):
                    signal.pidfd_send_signal(pidfd, signal.SIGKILL)
                    if not poll.poll(2000):
                        raise SystemExit(9)
            finally:
                os.close(pidfd)
        deadline = time.monotonic() + 2
        while group_still_has_members(pid):
            if time.monotonic() >= deadline:
                raise SystemExit(13)
            time.sleep(0.05)
        for name in ("backend.lock.json", nonce + ".log", nonce + ".token"):
            try:
                os.unlink(name, dir_fd=dirfd)
            except FileNotFoundError:
                pass
        print("CLEAN")
    finally:
        os.close(lockfd)
finally:
    os.close(dirfd)
