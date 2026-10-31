# Executed under update_gate.py's kernel lock by the remote system Python.
# Only the private token file path, never its contents, crosses argv.
import datetime
import fcntl
import hashlib
import json
import os
import stat
import subprocess
import sys
import time


def reclaim_dead_reservation(dirfd):
    # This runs while holding the shared update mutex, so another compliant
    # launcher cannot replace the reservation between inspection and removal.
    try:
        reservation = os.open(
            ".connect.lock",
            os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
            dir_fd=dirfd,
        )
    except (FileNotFoundError, NotADirectoryError):
        return False
    try:
        try:
            fd = os.open(
                "owner", os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), dir_fd=reservation
            )
            with os.fdopen(fd, "rb") as stream:
                raw = stream.read(128).decode("ascii")
            pid_text, marker_nonce = raw.split(":")
            if (
                not pid_text.isdigit()
                or int(pid_text) <= 0
                or len(marker_nonce) != 16
                or any(c not in "0123456789abcdef" for c in marker_nonce)
            ):
                return False
            try:
                os.kill(int(pid_text), 0)
                return False
            except ProcessLookupError:
                pass
            except (PermissionError, OverflowError):
                return False
            # Unknown entries prevent rmdir, and are never recursively removed.
            if os.listdir(reservation) != ["owner"]:
                return False
            os.unlink("owner", dir_fd=reservation)
        except (OSError, UnicodeError, ValueError):
            return False
    finally:
        os.close(reservation)
    try:
        os.rmdir(".connect.lock", dir_fd=dirfd)
        return True
    except OSError:
        return False


launcher, owner, nonce, profile, home, supervisor = sys.argv[1:]
owner_arg = owner
owner = os.path.expanduser(owner)
if len(nonce) != 16 or any(c not in "0123456789abcdef" for c in nonce):
    raise SystemExit(2)
os.makedirs(owner, mode=0o700, exist_ok=True)
dirfd = os.open(
    owner, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
)
try:
    st = os.fstat(dirfd)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid != os.getuid():
        raise SystemExit(3)
    os.fchmod(dirfd, 0o700)
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
                    raise SystemExit(4)
                time.sleep(0.02)
        # Share the retained Desktop's cross-process reservation namespace.
        deadline = time.monotonic() + 10
        while True:
            try:
                os.mkdir(".connect.lock", 0o700, dir_fd=dirfd)
                break
            except FileExistsError:
                if reclaim_dead_reservation(dirfd):
                    continue
                if time.monotonic() >= deadline:
                    raise SystemExit(11)
                time.sleep(0.05)
        reservation = os.open(
            ".connect.lock",
            os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
            dir_fd=dirfd,
        )
        try:
            ownerfd = os.open(
                "owner",
                os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
                0o600,
                dir_fd=reservation,
            )
            try:
                os.write(ownerfd, ("%s:%s" % (os.getpid(), nonce)).encode())
            finally:
                os.close(ownerfd)
            try:
                existing = os.open(
                    "backend.lock.json",
                    os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
                    dir_fd=dirfd,
                )
            except FileNotFoundError:
                existing = None
            if existing is not None:
                os.close(existing)
                print("EXISTING")
                sys.exit(0)
            token = os.path.join(owner, nonce + ".token")
            log = os.path.join(owner, nonce + ".log")
            args = [os.path.expanduser(launcher)]
            if profile:
                args += ["--profile", profile]
            args += [
                "serve",
                "--isolated",
                "--host",
                "127.0.0.1",
                "--port",
                "0",
                "--ssh-session-token-file",
                token,
                "--ssh-owner-nonce",
                nonce,
            ]
            logfd = os.open(
                nonce + ".log",
                os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
                0o600,
                dir_fd=dirfd,
            )
            env = dict(os.environ, HERMES_DESKTOP="1")
            try:
                child = subprocess.Popen(
                    [sys.executable, "-c", supervisor, *args],
                    stdin=subprocess.DEVNULL,
                    stdout=logfd,
                    stderr=logfd,
                    env=env,
                    start_new_session=True,
                    close_fds=True,
                )
            finally:
                os.close(logfd)
            try:
                # Linux start ticks bind the record to this PID incarnation. Other systems
                # remain fail-closed until their native creation identity is available.
                with open("/proc/%d/stat" % child.pid) as f:
                    raw = f.read()
                ticks = int(raw[raw.rfind(")") + 2 :].split()[19])
                with open(token, "rb") as f:
                    fingerprint = hashlib.sha256(f.read(4097)).hexdigest()[:32]
                started = datetime.datetime.now(datetime.timezone.utc).isoformat()
                record = {
                    "tokenFingerprint": fingerprint,
                    "startedAt": started,
                    "creationTime": "linux:" + str(ticks),
                    "schemaVersion": 2,
                    "protocolVersion": 1,
                    "supervisorProtocol": 1,
                    "ownershipId": os.path.basename(owner),
                    "spawnNonce": nonce,
                    "pid": child.pid,
                    "startTicks": ticks,
                    "port": 0,
                    "profile": profile,
                    "hermesPath": launcher,
                    "hermesHome": home,
                    "logPath": owner_arg + "/" + nonce + ".log",
                    "tokenPath": owner_arg + "/" + nonce + ".token",
                }
                tmp = nonce + ".lock.tmp"
                fd = os.open(
                    tmp,
                    os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
                    0o600,
                    dir_fd=dirfd,
                )
                try:
                    data = json.dumps(record, separators=(",", ":")).encode()
                    while data:
                        data = data[os.write(fd, data) :]
                    os.fsync(fd)
                finally:
                    os.close(fd)
                os.rename(tmp, "backend.lock.json", src_dir_fd=dirfd, dst_dir_fd=dirfd)
                print(child.pid, flush=True)
            except BaseException:
                # This exact unreaped child owns the process group; no remote PID guess.
                try:
                    os.killpg(child.pid, 9)
                except ProcessLookupError:
                    pass
                child.wait()
                raise
        finally:
            try:
                os.unlink("owner", dir_fd=reservation)
            except FileNotFoundError:
                pass
            os.close(reservation)
            os.rmdir(".connect.lock", dir_fd=dirfd)
    finally:
        os.close(lockfd)
finally:
    os.close(dirfd)
