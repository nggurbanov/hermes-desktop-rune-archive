#!/usr/bin/python3
"""Private SSH fixture: execute only exact, validated engine commands."""

import json
import os
import re
import shlex
import socket
import subprocess
import sys
import threading

root = os.path.dirname(os.path.dirname(os.path.realpath(__file__)))
with open(os.path.join(root, "ssh-manifest.json"), encoding="utf-8") as stream:
    manifest = json.load(stream)
launcher = manifest["launcher"]
machine_home = manifest["machine_home"]
marker = manifest["marker"]
port = manifest["port"]
codes = manifest["codes"]
os.environ.clear()
os.environ.update(HOME=root, HERMES_HOME=machine_home, PATH="/usr/bin:/bin", LC_ALL="C")


def reject():
    print("private SSH fixture rejected unexpected invocation", file=sys.stderr)
    raise SystemExit(64)


def q(value):
    return (
        "'"
        + "".join("'\\''" if c == "'" else "'\\\\'" if c == "\\" else c for c in value)
        + "'"
    )


def remote_path(value):
    if value.startswith("~/"):
        return '"$HOME"' + q(value[1:])
    if value.startswith("/"):
        return q(value)
    reject()


def actual(value):
    value = (
        value.replace("$HOME", root, 1)
        if value.startswith("$HOME/")
        else os.path.expanduser(value)
    )
    value = os.path.normpath(value)
    if os.path.commonpath((root, value)) != root:
        reject()
    return value


def owner_id(value):
    value = actual(value)
    prefix = os.path.join(machine_home, "desktop-ssh")
    if os.path.dirname(value) != prefix or not re.fullmatch(
        r"[0-9a-f]{32}", os.path.basename(value)
    ):
        reject()
    return os.path.basename(value)


def owner_expression(oid):
    relative = os.path.relpath(machine_home, root)
    return remote_path("~/" + relative + "/desktop-ssh/" + oid)


def file_expression(oid, name):
    relative = os.path.relpath(machine_home, root)
    return remote_path("~/" + relative + "/desktop-ssh/" + oid + "/" + name)


def run_python(code, args, input_data=None):
    result = subprocess.run(
        [sys.executable, "-c", code, *args], input=input_data, check=False
    )
    raise SystemExit(result.returncode)


def validate_payload(payload):
    try:
        args = shlex.split(payload)
    except ValueError:
        reject()
    if len(args) >= 3 and args[:2] == ["python3", "-c"]:
        code = args[2]
        if code == codes["spawn"] and len(args) == 9:
            _, _, _, requested_launcher, owner, nonce, profile, home, supervisor = args
            oid = owner_id(owner)
            if (
                requested_launcher != launcher
                or not re.fullmatch(r"[0-9a-f]{16}", nonce)
                or profile not in ("", "research", "dixie")
                or home != machine_home
                or supervisor != codes["supervisor"]
            ):
                reject()
            expected = "python3 -c " + " ".join(
                q(x)
                for x in (
                    code,
                    launcher,
                    "~/" + os.path.relpath(machine_home, root) + "/desktop-ssh/" + oid,
                    nonce,
                    profile,
                    home,
                    supervisor,
                )
            )
            if payload != expected:
                reject()
            return
        if code == codes["reclaim"] and len(args) == 8:
            _, _, _, owner, requested_launcher, home, profile, nonce = args
            oid = owner_id(owner)
            if (
                requested_launcher != launcher
                or home != machine_home
                or profile not in ("", "research", "dixie")
                or (nonce and not re.fullmatch(r"[0-9a-f]{16}", nonce))
            ):
                reject()
            expected = "python3 -c " + " ".join(
                q(x)
                for x in (
                    code,
                    "~/" + os.path.relpath(machine_home, root) + "/desktop-ssh/" + oid,
                    launcher,
                    home,
                    profile,
                    nonce,
                )
            )
            if payload != expected:
                reject()
            return
    # The only allowed shell compound fences one exact owner+nonce cleanup.
    oid_match = re.search(r"/desktop-ssh/([0-9a-f]{32})", payload)
    nonce_match = re.search(r"'([0-9a-f]{16})'; fi$", payload)
    if oid_match and nonce_match:
        owner = owner_expression(oid_match.group(1))
        nonce = nonce_match.group(1)
        expected = (
            "if [ ! -d "
            + owner
            + " ]; then printf CLEAN; else python3 -c "
            + q(codes["cleanup"])
            + " "
            + owner
            + " "
            + q(nonce)
            + "; fi"
        )
        if payload == expected:
            return
    reject()


def execute(command):
    if command == "uname -s; uname -m":
        print("Linux\nx86_64")
        return
    if command == "printf '%s' \"${HERMES_HOME-}\"":
        print(machine_home, end="")
        return
    if command == "printf '%s' \"$HOME\"":
        print(root, end="")
        return
    if command == "[ -x " + q(launcher) + " ] && printf OK || true":
        if os.access(launcher, os.X_OK):
            print("OK", end="")
        return
    if command == manifest["config_watchdog"]:
        result = subprocess.run(
            [launcher, "--profile", "default", "config", "path"],
            env={"HOME": root, "PATH": "/usr/bin:/bin", "LC_ALL": "C"},
            timeout=15,
            check=False,
        )
        raise SystemExit(result.returncode)
    if command == manifest["help_watchdog"]:
        result = subprocess.run([launcher, "serve", "--help"], timeout=15, check=False)
        raise SystemExit(result.returncode)
    gate_probe = "python3 - " + q(marker) + " '' " + q(launcher)
    if command == gate_probe:
        program = sys.stdin.buffer.read(65537)
        if program != codes["gate"].encode():
            reject()
        result = subprocess.run(
            [sys.executable, "-", marker, "", launcher], input=program, check=False
        )
        raise SystemExit(result.returncode)
    try:
        args = shlex.split(command)
    except ValueError:
        reject()
    if len(args) >= 3 and args[:2] == ["python3", "-c"]:
        code = args[2]
        if code == codes["gate"] and len(args) == 6:
            if actual(args[3]) != marker or actual(args[5]) != launcher:
                reject()
            validate_payload(args[4])
            expected = (
                "python3 -c "
                + q(code)
                + " "
                + q(marker)
                + " "
                + q(args[4])
                + " "
                + q(launcher)
            )
            if command != expected:
                reject()
            run_python(code, [marker, args[4], launcher])
        if (
            code == codes["discover"]
            and len(args) == 4
            and actual(args[3]) == machine_home
        ):
            if command != "python3 -c " + q(code) + " " + q(machine_home):
                reject()
            run_python(code, [machine_home])
        if code in (codes["upload"], codes["publish"], codes["cleanup"]):
            if code == codes["upload"] and len(args) == 4:
                token = actual(args[3])
                parent, name = os.path.split(token)
                oid = owner_id(parent)
                if not re.fullmatch(r"[0-9a-f]{16}\.token", name):
                    reject()
                expected = "python3 -c " + q(code) + " " + file_expression(oid, name)
                if command != expected:
                    reject()
                os.execv(sys.executable, [sys.executable, "-c", code, token])
            if code == codes["publish"] and len(args) == 7:
                oid = owner_id(args[3])
                nonce, published_port, fingerprint = args[4:]
                if (
                    not re.fullmatch(r"[0-9a-f]{16}", nonce)
                    or published_port != str(port)
                    or not re.fullmatch(r"[0-9a-f]{32}", fingerprint)
                ):
                    reject()
                expected = (
                    "python3 -c "
                    + q(code)
                    + " "
                    + owner_expression(oid)
                    + " "
                    + q(nonce)
                    + " "
                    + published_port
                    + " "
                    + q(fingerprint)
                )
                if command != expected:
                    reject()
                run_python(code, [actual(args[3]), nonce, published_port, fingerprint])
            if code == codes["cleanup"] and len(args) == 5:
                oid = owner_id(args[3])
                nonce = args[4]
                if not re.fullmatch(r"[0-9a-f]{16}", nonce):
                    reject()
                expected = (
                    "python3 -c "
                    + q(code)
                    + " "
                    + owner_expression(oid)
                    + " "
                    + q(nonce)
                )
                if command != expected:
                    reject()
                run_python(code, [actual(args[3]), nonce])
    oid_match = re.search(r"/desktop-ssh/([0-9a-f]{32})", command)
    if oid_match:
        owner = owner_expression(oid_match.group(1))
        inspect = "python3 -c " + q(codes["inspect"]) + " " + owner
        if (
            command
            == "if [ ! -e " + owner + " ]; then printf NONE; else " + inspect + "; fi"
        ):
            path = os.path.join(machine_home, "desktop-ssh", oid_match.group(1))
            if not os.path.exists(path):
                print("NONE", end="")
                return
            run_python(codes["inspect"], [path])
        log_match = re.search(r"/([0-9a-f]{16})\.log' 2>/dev/null \|\| true$", command)
        if log_match:
            nonce = log_match.group(1)
            expected = (
                "cat "
                + file_expression(oid_match.group(1), nonce + ".log")
                + " 2>/dev/null || true"
            )
            if command == expected:
                path = os.path.join(
                    machine_home, "desktop-ssh", oid_match.group(1), nonce + ".log"
                )
                try:
                    with open(path, "rb") as stream:
                        sys.stdout.buffer.write(stream.read(1048576))
                except FileNotFoundError:
                    pass
                return
    reject()


def forward(spec):
    match = re.fullmatch(r"127\.0\.0\.1:([0-9]+):127\.0\.0\.1:([0-9]+)", spec)
    if not match or int(match.group(2)) != port or not 0 < int(match.group(1)) < 65536:
        reject()
    sock = socket.socket()
    sock.bind(("127.0.0.1", int(match.group(1))))
    sock.listen(12)
    if os.path.exists("/proc/self/stat"):
        # This record belongs to the actual listener, before readiness is published.
        stat = open("/proc/self/stat", encoding="utf-8").read().rsplit(")", 1)[1].split()
        record = {"pid": os.getpid(), "start_ticks": int(stat[19]),
                  "pgid": os.getpgrp(), "local_port": int(match.group(1))}
        if os.path.exists(os.path.join(root, "forward-helper")):
            helper = subprocess.Popen(["/usr/bin/sleep", "60"])
            helper_stat = open(f"/proc/{helper.pid}/stat", encoding="utf-8").read().rsplit(")", 1)[1].split()
            record["helper"] = {"pid": helper.pid, "start_ticks": int(helper_stat[19]),
                                "pgid": int(helper_stat[2])}
        with open(os.path.join(root, "forward.json"), "w", encoding="utf-8") as stream:
            json.dump(record, stream)
    print(
        f"debug1: Local forwarding listening on 127.0.0.1 port {match.group(1)}.",
        file=sys.stderr,
        flush=True,
    )

    def pump(a, b):
        try:
            while True:
                data = a.recv(8192)
                if not data:
                    break
                b.sendall(data)
        except OSError:
            pass
        finally:
            try:
                b.shutdown(socket.SHUT_WR)
            except OSError:
                pass

    def connection(client):
        try:
            remote = socket.create_connection(("127.0.0.1", port))
        except OSError:
            client.close()
            return
        threading.Thread(target=pump, args=(client, remote), daemon=True).start()
        pump(remote, client)
        client.close()
        remote.close()

    while True:
        client, _ = sock.accept()
        threading.Thread(target=connection, args=(client,), daemon=True).start()


base = [
    "-o",
    "BatchMode=yes",
    "-o",
    "StrictHostKeyChecking=accept-new",
    "-o",
    "ControlMaster=no",
    "-o",
    "ControlPath=none",
    "-o",
    "ControlPersist=no",
    "-o",
    "ConnectTimeout=10",
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=3",
    "-o",
    "TCPKeepAlive=yes",
    "-p",
    "2222",
    "-l",
    "private",
]
args = sys.argv[1:]
if args[: len(base)] != base:
    reject()
rest = args[len(base) :]
if rest == ["-G", "--", "private.fixture"]:
    print("hostname private.fixture\nuser private\nport 2222")
elif (
    len(rest) == 8
    and rest[:5] == ["-v", "-o", "ExitOnForwardFailure=yes", "-N", "-L"]
    and rest[6:] == ["--", "private.fixture"]
):
    forward(rest[5])
elif len(rest) == 4 and rest[:3] == ["-T", "--", "private.fixture"]:
    execute(rest[3])
else:
    reject()
