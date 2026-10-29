import { existsSync, mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const moments = [
	{ composition: "Verevon-Plan-Moment", name: "plan", posterFrame: 140 },
	{ composition: "Verevon-Investigate-Moment", name: "investigate", posterFrame: 140 },
	{ composition: "Verevon-FollowUp-Moment", name: "follow-up", posterFrame: 160 },
];

const entryPoint = "remotion/index.ts";
const outputDirectory = resolve("public/verevon-product-moments");
const packageManagerPath = process.env.npm_execpath;
const fallbackPnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const macOSChrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const browserExecutable = process.env.REMOTION_BROWSER_EXECUTABLE
	?? (existsSync(macOSChrome) ? macOSChrome : undefined);

mkdirSync(outputDirectory, { recursive: true });

const runRemotion = (args) => {
	const command = packageManagerPath ? process.execPath : fallbackPnpm;
	const commandArguments = packageManagerPath
		? [packageManagerPath, "exec", "remotion", ...args]
		: ["exec", "remotion", ...args];
	const result = spawnSync(command, commandArguments, { stdio: "inherit" });

	if (result.status !== 0) {
		throw new Error(`Remotion command failed: ${args.join(" ")}`);
	}
};

const withBrowser = (args) => browserExecutable
	? [...args, `--browser-executable=${browserExecutable}`]
	: args;

for (const moment of moments) {
	const outputBase = resolve(outputDirectory, moment.name);

	runRemotion(withBrowser([
		"still",
		entryPoint,
		moment.composition,
		`${outputBase}-poster.jpg`,
		`--frame=${moment.posterFrame}`,
		"--image-format=jpeg",
		"--jpeg-quality=88",
	]));

	runRemotion(withBrowser([
		"render",
		entryPoint,
		moment.composition,
		`${outputBase}.mp4`,
		"--codec=h264",
		"--crf=20",
		"--muted",
		"--pixel-format=yuv420p",
	]));
}
