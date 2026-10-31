// Process-tree helpers: the harness must never leave chromium, vite or ffmpeg behind.
import { spawnSync } from "node:child_process";

const isWindows = process.platform === "win32";

/** All live processes as { pid, ppid, name }. */
export function listProcesses() {
	if (isWindows) {
		const ps = spawnSync(
			"powershell.exe",
			[
				"-NoProfile",
				"-NonInteractive",
				"-Command",
				"Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId),$($_.ParentProcessId),$($_.Name)\" }",
			],
			{ encoding: "utf8", windowsHide: true, timeout: 30_000 },
		);
		if (ps.status !== 0) return [];
		return ps.stdout
			.split(/\r?\n/)
			.filter(Boolean)
			.map((line) => {
				const [pid, ppid, ...name] = line.split(",");
				return { pid: Number(pid), ppid: Number(ppid), name: name.join(",") };
			});
	}
	const ps = spawnSync("ps", ["-A", "-o", "pid=,ppid=,comm="], { encoding: "utf8" });
	return ps.stdout
		.split("\n")
		.filter(Boolean)
		.map((line) => {
			const [pid, ppid, ...name] = line.trim().split(/\s+/);
			return { pid: Number(pid), ppid: Number(ppid), name: name.join(" ") };
		});
}

/** The process itself plus every descendant, as { pid, name }. */
export function processTree(rootPid, processes = listProcesses()) {
	const byParent = new Map();
	for (const p of processes) {
		if (!byParent.has(p.ppid)) byParent.set(p.ppid, []);
		byParent.get(p.ppid).push(p);
	}
	const root = processes.find((p) => p.pid === rootPid);
	const out = root ? [{ pid: root.pid, name: root.name }] : [{ pid: rootPid, name: "?" }];
	const queue = [rootPid];
	const seen = new Set(queue);
	while (queue.length) {
		const pid = queue.shift();
		for (const child of byParent.get(pid) ?? []) {
			if (seen.has(child.pid)) continue;
			seen.add(child.pid);
			out.push({ pid: child.pid, name: child.name });
			queue.push(child.pid);
		}
	}
	return out;
}

/** pid -> lower-case image name of every live process (tasklist on Windows: faster than CIM). */
function liveImageNames() {
	if (isWindows) {
		const r = spawnSync("tasklist", ["/FO", "CSV", "/NH"], { encoding: "utf8", windowsHide: true, timeout: 30_000 });
		if (r.status !== 0) return null;
		const map = new Map();
		for (const line of r.stdout.split(/\r?\n/)) {
			const cols = line.split('","');
			const pid = Number(cols[1]);
			if (Number.isFinite(pid)) map.set(pid, cols[0].replace(/^"/, "").toLowerCase());
		}
		return map;
	}
	return new Map(listProcesses().map((p) => [p.pid, p.name.toLowerCase()]));
}

/**
 * The subset of `pids` that is still alive. `names` (pid -> image name, or one image name for
 * all) guards against Windows PID reuse: a pid only counts as alive while it still belongs to the
 * same executable. If the process list cannot be read, every pid is reported alive.
 */
export function alivePids(pids, names) {
	if (!pids.length) return [];
	const live = liveImageNames();
	if (!live) return pids;
	const expected = (pid) => (typeof names === "string" ? names : names?.get?.(pid))?.toLowerCase();
	return pids.filter((pid) => {
		const name = live.get(pid);
		if (name === undefined) return false;
		const want = expected(pid);
		return !want || want === "?" || name === want;
	});
}

/** Polls until none of `pids` is alive or the timeout passes; returns the survivors. */
export async function waitGone(pids, timeoutMs = 20_000, names) {
	const deadline = Date.now() + timeoutMs;
	let survivors = alivePids(pids, names);
	while (survivors.length && Date.now() < deadline) {
		await new Promise((r) => setTimeout(r, 400));
		survivors = alivePids(survivors, names);
	}
	return survivors;
}

/** Force-kills a process and all of its descendants. Synchronous so it also works in 'exit'. */
export function killTree(pid) {
	if (!pid) return;
	if (isWindows) {
		spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], {
			stdio: "ignore",
			windowsHide: true,
			timeout: 30_000,
		});
		return;
	}
	try {
		process.kill(-pid, "SIGKILL");
	} catch {
		try {
			process.kill(pid, "SIGKILL");
		} catch {
			/* already gone */
		}
	}
}

/** Kills individual pids (used for any survivor after a tree kill). */
export function killPids(pids) {
	for (const pid of pids) {
		if (isWindows) {
			spawnSync("taskkill", ["/PID", String(pid), "/F"], { stdio: "ignore", windowsHide: true });
		} else {
			try {
				process.kill(pid, "SIGKILL");
			} catch {
				/* gone */
			}
		}
	}
}

/** The full command line of a process (Windows: CIM; elsewhere: /proc or ps). null if unknown. */
export function commandLineOf(pid) {
	if (!pid) return null;
	if (isWindows) {
		const r = spawnSync(
			"powershell.exe",
			["-NoProfile", "-NonInteractive", "-Command", `(Get-CimInstance Win32_Process -Filter "ProcessId=${Number(pid)}").CommandLine`],
			{ encoding: "utf8", windowsHide: true, timeout: 30_000 },
		);
		return r.status === 0 && r.stdout.trim() ? r.stdout.trim() : null;
	}
	const r = spawnSync("ps", ["-o", "args=", "-p", String(pid)], { encoding: "utf8" });
	return r.stdout.trim() || null;
}
