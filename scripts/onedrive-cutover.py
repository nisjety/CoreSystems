"""Plane cutover: repoint every OneDrive bind mount at the C:\\dev clone.

Recreates containers with `docker run` rather than compose, because several
planes' compose files refuse to interpolate without env vars that live nowhere
on disk (INGESTION_PG_PASSWORD and friends) -- the running containers hold the
only copies. So each container's full runtime config is read back from
`docker inspect` and replayed with the bind sources rewritten.

Replays, because losing any of these silently breaks things:
  image, entrypoint, cmd, user, init, restart policy, ALL networks with their
  ALIASES (the frontend reaches the gateway as `gateway`, a compose service
  alias -- losing it broke DNS earlier today), port bindings, environment,
  every mount (binds rewritten, volumes preserved by name), healthcheck.

Modes:
  verify  -- read-only. Lists each container's OneDrive binds and whether the
             clone already has that path. Changes nothing.
  plan    -- print the docker run argv per container. Changes nothing.
  apply   -- stop + rename old to <name>-onedrive-rollback, start the new one,
             wait for health, and roll back that one container if it fails.
"""
import json
import subprocess
import sys
import time
from pathlib import Path

OLD_ROOT = r"C:\Users\ImaFernandesDaCosta\OneDrive - Aquatiq\Dokumenter\CoresSystem"
# Docker Desktop reports some binds in a normalised Linux form instead of the
# Windows path that created them. Both denote the same host directory, and a
# matcher that knows only the Windows form silently skips those containers --
# five of them here, including both postgres init mounts.
OLD_ROOT_NORM = "/run/desktop/mnt/host/c/Users/ImaFernandesDaCosta/OneDrive - Aquatiq/Dokumenter/CoresSystem"
NEW_ROOT = r"C:\dev\CoresSystem"
ROLLBACK_SUFFIX = "-onedrive-rollback"


def docker(*args, check=True):
    r = subprocess.run(["docker", *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"docker {' '.join(args[:3])}: {r.stderr.strip()[:400]}")
    return r.stdout.strip()


def running_containers():
    ids = docker("ps", "-q").split()
    if not ids:
        return []
    raw = docker("inspect", *ids)
    return json.loads(raw)


def is_onedrive_src(src):
    s = (src or "").lower()
    return OLD_ROOT.lower() in s or OLD_ROOT_NORM.lower() in s


def onedrive_binds(spec):
    return [m for m in (spec.get("Mounts") or [])
            if m.get("Type") == "bind" and is_onedrive_src(m.get("Source"))]


def rewrite(src):
    """Map either host-path form onto the clone, always emitting a Windows path."""
    for root in (OLD_ROOT, OLD_ROOT_NORM):
        if src.lower().startswith(root.lower()):
            rel = src[len(root):].replace("/", "\\").lstrip("\\")
            return NEW_ROOT + "\\" + rel if rel else NEW_ROOT
    return src


def build_argv(spec):
    """docker run argv reproducing this container against the clone."""
    name = spec["Name"].lstrip("/")
    cfg, host, net = spec["Config"], spec["HostConfig"], spec["NetworkSettings"]
    argv = ["run", "-d", "--name", name]

    policy = (host.get("RestartPolicy") or {}).get("Name")
    if policy and policy != "no":
        argv += ["--restart", policy]
    if host.get("Init"):
        argv += ["--init"]
    if cfg.get("User"):
        argv += ["--user", cfg["User"]]
    if cfg.get("WorkingDir"):
        argv += ["-w", cfg["WorkingDir"]]

    # First network on the run itself (with aliases); the rest connected after.
    networks = net.get("Networks") or {}
    ordered = list(networks.items())
    if ordered:
        first, fcfg = ordered[0]
        argv += ["--network", first]
        for a in dict.fromkeys(fcfg.get("Aliases") or []):
            argv += ["--network-alias", a]

    for cport, bindings in (host.get("PortBindings") or {}).items():
        for b in bindings or []:
            hip, hport = b.get("HostIp") or "", b.get("HostPort") or ""
            argv += ["-p", f"{hip}:{hport}:{cport.split('/')[0]}" if hip else f"{hport}:{cport.split('/')[0]}"]

    for e in cfg.get("Env") or []:
        # PATH and the image's own build-time vars come from the image itself.
        if e.split("=", 1)[0] in ("PATH", "HOSTNAME", "HOME"):
            continue
        argv += ["-e", e]

    for m in spec.get("Mounts") or []:
        if m.get("Type") == "bind":
            src = m["Source"]
            # Must use the same both-forms test as detection. Checking only the
            # Windows form here (while detection knew both) recreated five
            # containers still pointing at OneDrive -- healthy, so it looked
            # like success.
            if is_onedrive_src(src):
                src = rewrite(src)
            argv += ["-v", f"{src}:{m['Destination']}" + ("" if m.get("RW", True) else ":ro")]
        elif m.get("Type") == "volume":
            nm = m.get("Name")
            # An unnamed (anonymous) volume is recreated empty by the image.
            argv += ["-v", (f"{nm}:{m['Destination']}" if nm else m["Destination"])]

    hc = cfg.get("Healthcheck") or {}
    test = hc.get("Test") or []
    if test and test[0] in ("CMD-SHELL", "CMD"):
        cmd = test[1] if test[0] == "CMD-SHELL" else " ".join(test[1:])
        argv += ["--health-cmd", cmd]
        for flag, key, div in (("--health-interval", "Interval", 1e9),
                               ("--health-timeout", "Timeout", 1e9),
                               ("--health-start-period", "StartPeriod", 1e9)):
            if hc.get(key):
                argv += [flag, f"{int(hc[key] / div)}s"]
        if hc.get("Retries"):
            argv += ["--health-retries", str(hc["Retries"])]

    ep = cfg.get("Entrypoint")
    if ep:
        argv += ["--entrypoint", ep[0]]

    argv += [cfg["Image"]]
    # Remaining entrypoint words must precede cmd, or they are lost.
    if ep and len(ep) > 1:
        argv += ep[1:]
    argv += cfg.get("Cmd") or []
    return name, argv, ordered[1:]


def health_of(name):
    try:
        return docker("inspect", "-f", "{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}", name)
    except RuntimeError:
        return "gone"


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "verify"
    only = sys.argv[2:] if len(sys.argv) > 2 else None

    targets = []
    for spec in running_containers():
        name = spec["Name"].lstrip("/")
        if name.endswith(ROLLBACK_SUFFIX):
            continue
        if only and name not in only:
            continue
        if onedrive_binds(spec):
            targets.append(spec)

    if mode == "verify":
        missing = 0
        for spec in sorted(targets, key=lambda s: s["Name"]):
            name = spec["Name"].lstrip("/")
            for m in onedrive_binds(spec):
                new = rewrite(m["Source"])
                ok = Path(new).exists()
                if not ok:
                    missing += 1
                print(f"  {'ok     ' if ok else 'MISSING'}  {name:38s} {new[len(NEW_ROOT) + 1:]}")
        print(f"\n  containers to cut over: {len(targets)}   missing paths in clone: {missing}")
        return 0 if missing == 0 else 1

    if mode == "plan":
        for spec in targets:
            name, argv, extra = build_argv(spec)
            print(f"\n--- {name} (extra networks: {[n for n, _ in extra]}) ---")
            print("  docker " + " ".join(f'"{a}"' if " " in a else a for a in argv[:40]))
        return 0

    if mode != "apply":
        print(f"unknown mode: {mode}")
        return 2

    ok, failed = [], []
    for spec in targets:
        name, argv, extra = build_argv(spec)
        had_health = bool((spec["Config"].get("Healthcheck") or {}).get("Test"))
        print(f"\n=== {name} ===", flush=True)
        try:
            docker("stop", name)
            # A rollback from an earlier pass already holds the original. Keep
            # it and discard this (equivalent) copy rather than failing on the
            # name collision or overwriting the only pristine snapshot.
            existing = docker("ps", "-aq", "-f", f"name=^{name}{ROLLBACK_SUFFIX}$", check=False)
            if existing:
                print(f"  rollback already exists; discarding the current copy")
                docker("rm", "-f", name)
            else:
                docker("rename", name, name + ROLLBACK_SUFFIX)
        except RuntimeError as e:
            print(f"  could not stage: {e}")
            failed.append((name, "stage"))
            continue
        try:
            docker(*argv)
            for netname, ncfg in extra:
                a = []
                for alias in dict.fromkeys(ncfg.get("Aliases") or []):
                    a += ["--alias", alias]
                docker("network", "connect", *a, netname, name)
            # Only wait when the container actually declares a healthcheck.
            status = "none"
            if had_health:
                for _ in range(40):
                    status = health_of(name)
                    if status not in ("starting",):
                        break
                    time.sleep(3)
            state = docker("inspect", "-f", "{{.State.Status}}", name)
            good = state == "running" and status in ("healthy", "none")
            if not good:
                raise RuntimeError(f"state={state} health={status}")
            print(f"  OK  state={state} health={status}")
            ok.append(name)
        except RuntimeError as e:
            print(f"  FAILED: {e}  -> rolling back this container")
            subprocess.run(["docker", "rm", "-f", name], capture_output=True, text=True)
            subprocess.run(["docker", "rename", name + ROLLBACK_SUFFIX, name], capture_output=True, text=True)
            subprocess.run(["docker", "start", name], capture_output=True, text=True)
            failed.append((name, str(e)[:120]))

    print(f"\n=== cut over: {len(ok)}   failed and rolled back: {len(failed)} ===")
    for n, why in failed:
        print(f"  FAILED {n}: {why}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
