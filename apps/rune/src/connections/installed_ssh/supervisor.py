"""Own one isolated remote serve process group for its complete lifetime."""

import os
import signal
import subprocess
import sys
import time

child = None
stopping = False


def stop_group(_signal=None, _frame=None):
    global stopping
    stopping = True
    if child is None:
        return
    # This supervisor is the session and group leader it created at spawn.
    # Keep that leader alive and ignore TERM while signaling the owned group.
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    try:
        os.killpg(os.getpgrp(), signal.SIGTERM)
    except ProcessLookupError:
        return
    # Signal handlers can interrupt subprocess.wait while it owns an internal
    # lock; do not re-enter wait from the handler.
    time.sleep(0.5)
    # The still-live leader pins this group identity through the final signal.
    os.killpg(os.getpgrp(), signal.SIGKILL)


signal.signal(signal.SIGTERM, stop_group)
child = subprocess.Popen(sys.argv[1:], close_fds=True)
if stopping:
    stop_group()
try:
    child.wait()
finally:
    # A serve exit can leave workers in its group; do not orphan them.
    stop_group()
