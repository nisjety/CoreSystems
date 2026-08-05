import { mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const films = [
	{
		composition: "Verevon-Build",
		name: "build",
		posterFrame: 83,
	},
	{
		composition: "Verevon-Connect",
		name: "connect",
		posterFrame: 149,
	},
	{
		composition: "Verevon-Ground",
		name: "ground",
		posterFrame: 221,
	},
	{
		composition: "Verevon-Approve",
		name: "approve",
		posterFrame: 317,
	},
];

const entryPoint = "remotion/index.ts";
const outputDirectory = resolve("public/feature-films");
const packageManagerPath = process.env.npm_execpath;
const fallbackPnpm =
	process.platform === "win32" ? "pnpm.cmd" : "pnpm";

mkdirSync(outputDirectory, { recursive: true });

const runRemotion = (args) => {
	const command = packageManagerPath
		? process.execPath
		: fallbackPnpm;
	const commandArguments = packageManagerPath
		? [packageManagerPath, "exec", "remotion", ...args]
		: ["exec", "remotion", ...args];
	const result = spawnSync(
		command,
		commandArguments,
		{ stdio: "inherit" },
	);

	if (result.status !== 0) {
		throw new Error(`Remotion command failed: ${args.join(" ")}`);
	}
};

for (const film of films) {
	const outputBase = resolve(outputDirectory, film.name);

	runRemotion([
		"still",
		entryPoint,
		film.composition,
		`${outputBase}-poster.jpg`,
		`--frame=${film.posterFrame}`,
		"--image-format=jpeg",
		"--jpeg-quality=88",
	]);

	runRemotion([
		"render",
		entryPoint,
		film.composition,
		`${outputBase}.mp4`,
		"--codec=h264",
		"--crf=19",
		"--muted",
		"--pixel-format=yuv420p",
	]);

	runRemotion([
		"render",
		entryPoint,
		film.composition,
		`${outputBase}.webm`,
		"--codec=vp9",
		"--crf=30",
		"--muted",
		"--pixel-format=yuv420p",
	]);
}
