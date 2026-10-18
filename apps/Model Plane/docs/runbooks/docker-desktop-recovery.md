# Docker Desktop recovery (Windows)

Owner: Model Plane operator. Script: `apps/Model Plane/scripts/dev/docker-desktop-recover.ps1` (a dry run unless `-Apply` is given).

Who runs this: the human operator or the lead only. Automated agents never run the `-Apply` path, the recovery script, the disposable-Postgres harness (`apps/Model Plane/scripts/tests/session-core-db-tests.sh`), or any `docker` command on this machine. Cargo runs on the host only.

Context: On 2026-10-17 the Docker Desktop VM wedged twice. The first time the backend log showed `connect tcp 192.168.65.7:2376: no route to host`. The second time the WSL VM load average was about 468 with 94 MB free, because dozens of `rustc` and `rust-lld` processes from an orphaned Rust build container were still running and no Windows process owned them. Each forced restart then failed on stale socket folders that Windows cannot delete (error 1920). The fix below worked on 2026-10-11 and 2026-10-12. It renames the folders and never deletes them.

## Symptoms

- `docker` commands hang, or fail with a connection error.
- The Docker logs contain `no route to host` for 192.168.65.x:2376.
- A restart fails with `initializing Secrets Engine: listening on unix://...docker-secrets-engine\engine.sock: remove ...: The file cannot be accessed by the system`, or with `initializing Ingest server: ...Docker\run\sailor-ingest.sock: remove ...`.
- Container work stops, the VM load stays high, and free memory is low.

## Diagnose (read-only; every command has a time limit)

Run these in PowerShell 7. If a command hangs for more than about 30 seconds, press Ctrl+C and move on. A hang is itself a symptom.

1. Engine: `docker version --format '{{.Server.Version}}'`. No answer means the engine is down or wedged.
2. Processes: `Get-Process | Where-Object { $_.ProcessName -match '^(Docker Desktop|com\.docker\..+|vpnkit.*)$' } | Select-Object ProcessName, Id, StartTime`
3. Distro: `$env:WSL_UTF8 = '1'; wsl.exe -l -v`. Read the `docker-desktop` state. Do not start it just to look.
4. Only if it says Running: `wsl.exe -d docker-desktop --exec cat /proc/loadavg` and `wsl.exe -d docker-desktop --exec grep MemAvailable /proc/meminfo`. A load in the tens or hundreds, or MemAvailable under about 300 MB, means an overloaded VM.
5. Build processes in the VM, only if Running: `wsl.exe -d docker-desktop --exec sh -c "grep -l -x -E 'rustc|rust-lld|cargo' /proc/[0-9]*/comm 2>/dev/null | wc -l"`. A large count with no matching container is the orphan pattern.
6. Host free space: `Get-PSDrive -PSProvider FileSystem | Select-Object Name, @{n='FreeGB';e={[math]::Round($_.Free/1GB,1)}}`. The VM disk cannot grow when the host volume is full.
7. Logs: `Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Docker\log') -Recurse -File -Filter '*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 5`, then `Select-String -LiteralPath <file> -Pattern 'no route to host|initializing Secrets Engine|initializing Ingest server|error 1920|cannot be accessed by the system'`.
8. The script's dry run: `pwsh -NoProfile -File "apps/Model Plane/scripts/dev/docker-desktop-recover.ps1"`. It changes nothing. Exit 0 means the report is complete; exit 3 means Docker Desktop.exe was not found.

## Fix

1. Read the dry-run report. Check that the process names, the folder paths and the Docker Desktop.exe path are the ones you expect.
2. If the engine still answers and a build container is running, find that container first (see the orphan section) and stop it by ID. If the engine does not answer, skip this step; the restart stops the VM's processes.
3. Run `pwsh -NoProfile -File "apps/Model Plane/scripts/dev/docker-desktop-recover.ps1" -Apply`. Add `-Force` only if the engine answers but cannot be used, because step 2 of the script then stops every running container.
4. The script stops the Docker Desktop processes, terminates the `docker-desktop` distro, renames `%LOCALAPPDATA%\docker-secrets-engine` and `%LOCALAPPDATA%\Docker\run` to `*.stale-<timestamp>`, starts Docker Desktop, and waits up to five minutes for `docker info`.
5. Expect every container in the VM to stop. Named volumes survive. Containers with a restart policy return with the engine.
6. Restart the Model Plane stack with its launcher: `cd "apps/Model Plane" && ./scripts/compose.sh up -d`, as `apps/Model Plane/CLAUDE.md` says. The compose file header names `./build-verevon-services.sh --from model` for builds; that script is not in this worktree, so use `compose.sh` unless the lead says otherwise. Never use a bare `docker compose up`.
7. Delete the `*.stale-*` folders only after a working restart, by hand, on that exact path. They may need an elevated shell.

Manual equivalent, if the script is unavailable: quit Docker Desktop from the tray; `wsl.exe --terminate docker-desktop`; rename the two folders to `*.stale-<timestamp>`; start `%LOCALAPPDATA%\Programs\DockerDesktop\Docker Desktop.exe`; wait for `docker info`.

## If the engine still does not start

- Read the log lines the script printed. Do not reset.
- In an elevated PowerShell, check the Docker service with `Get-Service *docker*`. The script cannot stop a SYSTEM service without elevation, and it says so when that happens.
- Free host disk space before anything else if the host volume is near full.
- Escalate to the user. `wsl.exe --shutdown` stops every WSL distro, including unrelated work, so it needs the user's explicit approval.

## What not to do

- Do not click "Reset to factory defaults" in Docker Desktop. It deletes every container, image and volume, including the Postgres data of every plane.
- Do not run `wsl --unregister docker-desktop` or `wsl --unregister docker-desktop-data`, and do not delete the Docker Desktop WSL data folder under `%LOCALAPPDATA%\Docker\wsl`.
- Do not run `docker system prune --volumes`, `docker volume prune` or `docker builder prune` as a recovery step. Volumes hold the database data, and prune on a full disk does not free space safely.
- Do not run `wsl.exe --shutdown` as the first step.
- Do not delete the stale socket folders. Rename them; Windows cannot remove their reparse points (error 1920).
- Do not start a Rust build container to run tests on this machine. Run cargo on the host, one crate at a time. Containers need the lead's explicit go-ahead.

## How to spot an orphaned build container

An orphan keeps building or testing after the command that started it has exited.

- List: `docker ps --format "table {{.ID}}\t{{.Image}}\t{{.RunningFor}}\t{{.Names}}"`. Suspects are `rust:*` images, names you do not recognise, no `com.docker.compose.project` label, and an age of hours with no matching terminal.
- Inspect: `docker inspect --format "{{.Config.Cmd}} {{.State.StartedAt}}" <id>`. A `cargo` or `sh -c "cargo ..."` command with no owner is an orphan.
- Find its process in the VM with the step 5 pattern, which prints the PID in the path, then show the command line: `wsl.exe -d docker-desktop --exec sh -c "tr '\0' ' ' < /proc/<pid>/cmdline"`.
- Stop only by ID, after inspect shows the command: `docker stop <id>`. Remove it with `docker rm <id>` only once you have decided its state can be lost. Never stop by image pattern, and never prune.

## Prevent recurrence

- The Model Plane plan tells people to run Linux-gated Rust tests in a `rust:1` container on Windows (`docs/plans/model-plane-chat-build-plan-2026-10-14.md`, for example lines 1106 and 2108; the same advice appears at lines 646, 1108, 1110 and 1243). That is a likely origin of the orphan, not a confirmed one. Until those instructions carry limits, run cargo on the host, one crate at a time. A container run needs the lead's explicit go-ahead.
- Run one build at a time in the VM, and check `docker stats --no-stream` before starting another.
- The VM memory limit is a user setting (Docker Desktop, Settings, Resources; or `[wsl2] memory=` in `%UserProfile%\.wslconfig`). Scripts in this repo do not change it.

## Verify after recovery

- `docker info` answers with a server version.
- `docker ps` lists the expected stack containers.
- `wsl.exe -d docker-desktop --exec cat /proc/loadavg` shows a load near idle.
- `docker exec model-plane-postgres-1 pg_isready -U postgres` reports that it is accepting connections.

Related: the 2026-10-17 WS03 S5 live proof records these incidents (`docs/compliance/evidence/live-proof-ws03-s5-effect-ledger-2026-10-17.txt`).
