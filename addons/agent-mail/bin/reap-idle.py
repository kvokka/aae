"""Run the mailbox-wide idle-agent sweep opportunistically before the server opens the database."""

import fcntl
import os
from pathlib import Path
import subprocess
import sys
import time


def reap(data):
    mailbox = data / "mailbox"
    database = mailbox / "storage.sqlite3"

    # Keep the inode in place. Exiting, including on a crash, releases the lock.
    with (data / "runtime" / "last-reap").open("a+") as stamp:
        try:
            fcntl.flock(stamp, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        stamp.seek(0)
        last = stamp.read().strip()
        if last.isdecimal() and 0 <= time.time() - int(last) < 3600:
            return

        subprocess.run(
            ["mcp-agent-mail", "agents", "reap", "--stale-days", "1"],
            env=dict(
                os.environ,
                AM_INTERFACE_MODE="cli",
                STORAGE_ROOT=str(mailbox),
                DATABASE_URL=f"sqlite:///{database}",
            ),
            cwd=mailbox,
            stdout=subprocess.DEVNULL,
            timeout=5,
            check=True,
        )
        stamp.seek(0)
        stamp.truncate()
        stamp.write(str(int(time.time())))


if __name__ == "__main__":
    try:
        reap(Path(sys.argv[1]).resolve())
    except (OSError, subprocess.SubprocessError) as error:
        sys.exit(f"agent-mail: idle-agent cleanup: {error}")
