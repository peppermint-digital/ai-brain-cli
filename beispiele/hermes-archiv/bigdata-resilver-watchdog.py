#!/usr/bin/env python3
import os
import re
import subprocess
import sys

HOST = "192.168.1.35"
USER = "infra-monitor"
KEY = "/home/codingmachine/.ssh/infra-monitor"
POOL = "BIG DATA"
DONE_MARKER = "/tmp/hermes-bigdata-resilver-done"
OLD_GUID = "6185620516017788858"
NEW_GUID = "6883474465352996420"


def ssh(cmd, timeout=60):
    return subprocess.run([
        "ssh", "-i", KEY, "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
        f"{USER}@{HOST}", cmd,
    ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)

p = ssh(f"sudo -n /usr/sbin/zpool status -v -g {POOL!r}", timeout=90)
if p.returncode != 0:
    print(f"ALERT BIG DATA resilver: SSH/zpool check failed: {p.stderr.strip() or p.stdout.strip()}")
    sys.exit(0)

status = p.stdout
alerts = []

state_match = re.search(r"\n state:\s+(\S+)", status)
state = state_match.group(1) if state_match else "UNKNOWN"
if state in {"FAULTED", "UNAVAIL", "SUSPENDED"}:
    alerts.append(f"Pool state is {state}.")
elif state not in {"DEGRADED", "ONLINE"}:
    alerts.append(f"Unexpected pool state: {state}.")

if "errors: No known data errors" not in status:
    alerts.append("ZFS no longer reports 'No known data errors'.")

resilvering = "resilver in progress" in status
replacing = "replacing-" in status or OLD_GUID in status

if alerts:
    print("ALERT BIG DATA resilver:\n" + "\n".join(f"- {a}" for a in alerts))
    sys.exit(0)

if resilvering:
    # Expected noisy state; stay silent.
    sys.exit(0)

if state == "ONLINE" and not replacing:
    if not os.path.exists(DONE_MARKER):
        open(DONE_MARKER, "w").write("done\n")
        print("BIG DATA Resilver fertig: Pool ist ONLINE und keine replacing-Topology mehr sichtbar.")
    sys.exit(0)

# Not actively resilvering but still not clean.
summary = []
for line in status.splitlines():
    if any(token in line for token in ["state:", "scan:", "DEGRADED", "UNAVAIL", "replacing-", OLD_GUID, NEW_GUID, "errors:"]):
        summary.append(line)
print("ALERT BIG DATA resilver: Resilver läuft nicht, Pool aber noch nicht sauber:\n" + "\n".join(summary[:40]))
