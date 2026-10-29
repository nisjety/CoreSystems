// Starts the SPA's Vite dev server in scene mode and guarantees it is stopped again.
import { spawn } from "node:child_process";
import { connect } from "node:net";
import { CaptureError, SPA_ROOT, sleep } from "./util.mjs";
import { killPids, killTree, processTree, waitGone } from "./processes.mjs";

/** True when something accepts TCP connections on host:port. */
export function isPortOpen(port, host = "127.0.0.1", timeoutMs = 750) {
	return new Promise((resolvePort) => {
		const socket = connect({ port, host });
		const done = (open) => {
			socket.destroy();
			resolvePort(open);
		};
		socket.setTimeout(timeoutMs, () => done(false));
		socket.once("connect", () => done(true));
		socket.once("error", () => done(false));
	});
}

/** Polls `url` until it answers with an HTTP status below 500. */
export async function waitForHttp(url, { timeoutMs, isDead } = {}) {
	const deadline = Date.now() + timeoutMs;
	let lastError = "no response";
	while (Date.now() < deadline) {
		if (isDead?.()) return { ok: false, reason: "process exited" };
		try {
			const res = await fetch(url, { signal: AbortSignal.timeout(5000) });
			if (res.status < 500) return { ok: true, status: res.status };
			lastError = `HTTP ${res.status}`;
		} catch (error) {
			lastError = error.cause?.code || error.message;
		}
		await sleep(300);
	}
	return { ok: false, reason: `timed out after ${timeoutMs} ms (${lastError})` };
}

/**
 * Starts `pnpm exec vite` in the SPA with VITE_SCENE_MODE=1.
 * Returns { url, pid, stop(), tree() }. stop() is idempotent and synchronous-safe.
 */
export async function startDevServer({ port, log, timeoutMs = 120_000 }) {
	if (await isPortOpen(port)) {
		throw new CaptureError(
			`Port ${port} is already in use. Pass --url to capture from a running server, or --port to pick a free port.`,
		);
	}
	const command = `pnpm exec vite --host 127.0.0.1 --port ${port} --strictPort`;
	log.info(`starting dev server: (cd ${SPA_ROOT}) VITE_SCENE_MODE=1 ${command}`);
	const child = spawn(command, {
		cwd: SPA_ROOT,
		shell: true,
		windowsHide: true,
		detached: process.platform !== "win32",
		env: { ...process.env, VITE_SCENE_MODE: "1", BROWSER: "none", FORCE_COLOR: "0" },
		stdio: ["ignore", "pipe", "pipe"],
	});
	const output = [];
	const keep = (chunk) => {
		for (const line of chunk.toString().split(/\r?\n/)) {
			if (line.trim()) output.push(line);
		}
		if (output.length > 200) output.splice(0, output.length - 200);
	};
	child.stdout.on("data", keep);
	child.stderr.on("data", keep);
	let exited = false;
	let exitCode = null;
	child.once("exit", (code) => {
		exited = true;
		exitCode = code;
	});

	let knownTree = [{ pid: child.pid, name: "shell" }];
	let stopped = false;
	const server = {
		url: `http://127.0.0.1:${port}`,
		pid: child.pid,
		command,
		output,
		/** Remembers every process the server spawned, so stop() can prove none survived. */
		refreshTree() {
			if (!exited) {
				const tree = processTree(child.pid);
				const seen = new Set(knownTree.map((p) => p.pid));
				for (const p of tree) if (!seen.has(p.pid)) knownTree.push(p);
			}
			return knownTree;
		},
		/** Kills the whole tree and resolves to the pids still alive afterwards (should be []). */
		async stop() {
			if (stopped) return [];
			stopped = true;
			killTree(child.pid);
			const names = new Map(knownTree.map((p) => [p.pid, p.name]));
			let survivors = await waitGone(knownTree.map((p) => p.pid), 20_000, names);
			if (survivors.length) {
				killPids(survivors);
				survivors = await waitGone(survivors, 10_000, names);
			}
			return survivors;
		},
		/** Synchronous last-resort kill for process 'exit' handlers. */
		killNow() {
			stopped = true;
			killTree(child.pid);
		},
	};

	const ready = await waitForHttp(`${server.url}/`, { timeoutMs, isDead: () => exited });
	if (!ready.ok) {
		server.refreshTree();
		await server.stop();
		throw new CaptureError(
			`Dev server did not come up on port ${port}: ${ready.reason}${exited ? ` (exit code ${exitCode})` : ""}.\n` +
				`Last output:\n  ${output.slice(-25).join("\n  ")}`,
		);
	}
	server.refreshTree();
	log.info(`dev server ready at ${server.url} (pids ${knownTree.map((p) => p.pid).join(", ")})`);
	return server;
}
