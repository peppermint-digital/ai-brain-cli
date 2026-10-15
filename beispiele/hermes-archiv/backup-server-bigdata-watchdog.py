#!/usr/bin/env python3
import subprocess, re, sys

HOST = "192.168.1.35"
USER = "infra-monitor"
KEY = "/home/codingmachine/.ssh/infra-monitor"
POOL = "BIG DATA"
EXPECTED_FAULT_GUID = "6185620516017788858"  # ZL2PTBRA, known failed disk
CRITICAL_SERIALS = {"ZL2PGEZA", "ZL2PSWA9", "ZL2PFCWQ"}  # remaining disks in same RAIDZ1 vdev

def ssh(cmd, timeout=60):
    return subprocess.run([
        "ssh", "-i", KEY, "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
        f"{USER}@{HOST}", cmd
    ], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)

alerts = []

p = ssh(f"sudo -n /usr/sbin/zpool status -v -g {repr(POOL)}", timeout=90)
if p.returncode != 0:
    print(f"ALERT backup-server {HOST}: SSH/zpool check failed: {p.stderr.strip() or p.stdout.strip()}")
    sys.exit(0)

status = p.stdout
if "errors: No known data errors" not in status:
    alerts.append("ZFS reports data errors or missing 'No known data errors'.")

state_match = re.search(r"\n state:\s+(\S+)", status)
state = state_match.group(1) if state_match else "UNKNOWN"
if state in {"FAULTED", "UNAVAIL", "SUSPENDED"}:
    alerts.append(f"Pool state is {state}.")
elif state not in {"DEGRADED", "ONLINE"}:
    alerts.append(f"Unexpected pool state: {state}.")

faulted_lines = [ln.strip() for ln in status.splitlines() if re.search(r"\bFAULTED\b", ln)]
# Expected current state: exactly one faulted device, the known GUID ZL2PTBRA.
if len(faulted_lines) != 1 or EXPECTED_FAULT_GUID not in faulted_lines[0]:
    alerts.append("Faulted-device set changed: " + " | ".join(faulted_lines[:6]))

# Check SMART on the three remaining disks in the degraded RAIDZ1 vdev.
p = ssh("sudo -n lsblk -dn -o NAME,SERIAL | awk '{print $1\" \"$2}'", timeout=60)
if p.returncode == 0:
    serial_to_dev = {}
    for line in p.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            serial_to_dev[parts[1]] = "/dev/" + parts[0]
    for serial in sorted(CRITICAL_SERIALS):
        dev = serial_to_dev.get(serial)
        if not dev:
            alerts.append(f"Critical sibling disk missing: {serial}")
            continue
        sp = ssh(f"sudo -n smartctl -H -A {dev}", timeout=60)
        if sp.returncode != 0:
            alerts.append(f"SMART check failed for {serial} ({dev})")
            continue
        out = sp.stdout
        if "PASSED" not in out and "OK" not in out:
            alerts.append(f"SMART overall not PASSED for {serial} ({dev})")
        def val(attr):
            m = re.search(rf"^\s*\d+\s+{attr}\s+.*?\s(-?\d+)\s*(?:\(|$)", out, re.M)
            return int(m.group(1)) if m else 0
        realloc = val("Reallocated_Sector_Ct")
        pending = val("Current_Pending_Sector")
        offline = val("Offline_Uncorrectable")
        if realloc or pending or offline:
            alerts.append(f"SMART sectors on critical sibling {serial} ({dev}): realloc={realloc}, pending={pending}, offline={offline}")
else:
    alerts.append("Could not map disk serials via lsblk.")

if alerts:
    print("ALERT backup-server BIG DATA watchdog:\n" + "\n".join(f"- {a}" for a in alerts))
