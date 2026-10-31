import fcntl
import json
import os
import stat
import sys
import time

owner, nonce, port, fingerprint = sys.argv[1:]
owner = os.path.expanduser(owner)
port = int(port)
if (
    len(nonce) != 16
    or any(c not in "0123456789abcdef" for c in nonce)
    or len(fingerprint) != 32
    or any(c not in "0123456789abcdef" for c in fingerprint)
    or not 0 < port < 65536
):
    raise SystemExit(2)
dirfd = os.open(
    owner, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
)
try:
    st = os.fstat(dirfd)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid != os.getuid():
        raise SystemExit(3)
    lockfd = os.open(
        "reservation.lock", os.O_RDWR | getattr(os, "O_NOFOLLOW", 0), dir_fd=dirfd
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
        fd = os.open(
            "backend.lock.json",
            os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
            dir_fd=dirfd,
        )
        with os.fdopen(fd, "rb") as f:
            record = json.loads(f.read(65537))
        if (
            record.get("schemaVersion") != 2
            or record.get("spawnNonce") != nonce
            or record.get("ownershipId") != os.path.basename(owner)
        ):
            raise SystemExit(5)
        pid = record.get("pid")
        ticks = record.get("startTicks")
        if not isinstance(ticks, int):
            creation = record.get("creationTime", "")
            ticks = (
                int(creation[6:])
                if isinstance(creation, str)
                and creation.startswith("linux:")
                and creation[6:].isdigit()
                else None
            )
        if not isinstance(pid, int) or pid <= 0 or not isinstance(ticks, int):
            raise SystemExit(6)
        with open("/proc/%d/stat" % pid) as f:
            raw = f.read()
        if int(raw[raw.rfind(")") + 2 :].split()[19]) != ticks:
            raise SystemExit(7)
        record["port"] = port
        record["tokenFingerprint"] = fingerprint
        tmp = nonce + ".publish.tmp"
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
        print("PUBLISHED")
    finally:
        os.close(lockfd)
finally:
    os.close(dirfd)
