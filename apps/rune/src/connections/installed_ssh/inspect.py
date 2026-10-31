# A lock record is only a candidate; require the exact live process incarnation
# and the nonce-specific arguments before allowing a tunnel/proof attempt.
import json
import os
import stat
import sys

owner = sys.argv[1]
owner_expanded = os.path.expanduser(owner)
dirfd = os.open(
    owner_expanded,
    os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
)
try:
    st = os.fstat(dirfd)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid != os.getuid():
        raise SystemExit(2)
    try:
        fd = os.open(
            "backend.lock.json",
            os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
            dir_fd=dirfd,
        )
    except FileNotFoundError:
        print("NONE")
        sys.exit(0)
    with os.fdopen(fd, "rb") as f:
        data = f.read(65537)
    if len(data) > 65536:
        raise SystemExit(3)
    record = json.loads(data)
    nonce = record.get("spawnNonce")
    pid = record.get("pid")
    ticks = record.get("startTicks")
    profile = record.get("profile")
    if (
        record.get("schemaVersion") != 2
        or record.get("protocolVersion") != 1
        or record.get("ownershipId") != os.path.basename(owner_expanded)
        or not isinstance(nonce, str)
        or len(nonce) != 16
        or any(c not in "0123456789abcdef" for c in nonce)
        or not isinstance(pid, int)
        or pid <= 0
        or not isinstance(profile, str)
    ):
        raise SystemExit(4)
    with open("/proc/%d/stat" % pid) as f:
        raw = f.read()
    actual = int(raw[raw.rfind(")") + 2 :].split()[19])
    creation = record.get("creationTime")
    if isinstance(ticks, int) and actual != ticks:
        raise SystemExit(5)
    if (
        isinstance(creation, str)
        and creation.startswith("linux:")
        and creation != "linux:" + str(actual)
    ):
        raise SystemExit(5)
    with open("/proc/%d/cmdline" % pid, "rb") as f:
        args = [os.fsdecode(x) for x in f.read().split(b"\0") if x]
    token = os.path.join(owner_expanded, nonce + ".token")
    try:
        serve = [
            i
            for i, a in enumerate(args)
            if a == "serve" and not (i > 0 and args[i - 1] == "--profile")
        ]
        valid = (
            len(serve) == 1
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
        valid = False
    if not valid:
        raise SystemExit(6)
    print(json.dumps(record, separators=(",", ":")))
finally:
    os.close(dirfd)
