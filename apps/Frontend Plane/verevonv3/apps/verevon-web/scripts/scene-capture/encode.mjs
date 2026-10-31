// ffmpeg side of the harness. Raw PNG frames go in through stdin; nothing large touches the disk.
//
//   capture loop --PNG 2560x1600--> [downscaler: crop + lanczos] --rgb24 1280x800--> node
//   node: output frame k = render k, except the last DISSOLVE_FRAMES frames, which are blended
//         linearly into render frames 0..D-1 (integer math, exact weights j/D, j = 1..D)
//   node --rgb24--> [encoder: bt709 yuv420p] --> <id>.mp4 (h264) and <id>.webm (vp9)
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { CaptureError, DISSOLVE_FRAMES, FPS, OUT_HEIGHT, OUT_WIDTH } from "./util.mjs";

const FRAME_BYTES = OUT_WIDTH * OUT_HEIGHT * 3;
const SIZE = `${OUT_WIDTH}x${OUT_HEIGHT}`;
// `setparams` tags the frames themselves: with ffmpeg 9 the encoders take primaries and transfer
// from the frame, so the -color_* output options alone left both 'unknown' in the container.
const TO_BT709 = `scale=out_color_matrix=bt709:out_range=tv:flags=accurate_rnd+full_chroma_int,format=yuv420p,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv`;
const COLOR_TAGS = ["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"];

export function ffmpegVersion() {
	const r = spawnSync("ffmpeg", ["-hide_banner", "-version"], { encoding: "utf8", windowsHide: true });
	if (r.error || r.status !== 0) {
		throw new CaptureError("ffmpeg is not on PATH (needed for encoding). Install it or pass --no-encode.");
	}
	return r.stdout.split(/\r?\n/)[0];
}

function startFfmpeg(args, label) {
	const child = spawn("ffmpeg", args, { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
	const stderr = [];
	child.stderr.on("data", (d) => {
		stderr.push(d.toString());
		if (stderr.length > 400) stderr.splice(0, stderr.length - 400);
	});
	const exited = new Promise((resolveExit) => {
		child.once("error", (error) => resolveExit({ code: -1, error }));
		child.once("exit", (code) => resolveExit({ code }));
	});
	// A broken pipe surfaces through `exited`; avoid unhandled stream errors.
	child.stdin.on("error", () => {});
	return {
		child,
		label,
		args,
		async done() {
			const { code, error } = await exited;
			if (code !== 0) {
				throw new CaptureError(
					`ffmpeg (${label}) failed with exit code ${code}${error ? `: ${error.message}` : ""}\n${stderr.join("").slice(-3000)}`,
				);
			}
		},
		stderrText: () => stderr.join(""),
		kill() {
			try {
				child.kill("SIGKILL");
			} catch {
				/* gone */
			}
		},
	};
}

async function write(stream, buf) {
	if (!stream.write(buf)) await once(stream, "drain");
}

/**
 * The downscaler + loop composer + encoder chain.
 * push(png) feeds one raw frame; finish() resolves when every output file is closed.
 */
export class EncodePipeline {
	constructor({ frames, rawSize, mp4Path, webmPath, posterFrame, log, encode = true }) {
		this.frames = frames;
		this.dissolve = Math.min(DISSOLVE_FRAMES, Math.floor(frames / 2));
		this.posterFrame = posterFrame;
		this.posterRgb = null;
		this.head = []; // render frames 0..D-1 (rgb24, 1280x800)
		this.received = 0;
		this.log = log;
		this.encode = encode;
		const crop =
			rawSize.width !== 2 * OUT_WIDTH || rawSize.height !== 2 * OUT_HEIGHT
				? `crop=${Math.min(rawSize.width, 2 * OUT_WIDTH)}:${Math.min(rawSize.height, 2 * OUT_HEIGHT)}:0:0,`
				: "";
		this.filters = { downscale: `${crop}scale=${OUT_WIDTH}:${OUT_HEIGHT}:flags=lanczos+accurate_rnd+full_chroma_int`, toBt709: TO_BT709 };
		this.downscaler = startFfmpeg(
			[
				"-hide_banner", "-loglevel", "error",
				"-f", "image2pipe", "-framerate", String(FPS), "-c:v", "png", "-i", "pipe:0",
				"-vf", this.filters.downscale,
				"-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1",
			],
			"downscale",
		);
		// The last output frame equals render frame D-1, which equals the resting frame 0. Encoding it
		// as a keyframe too makes the encoded 479 and 0 nearly identical (no P-frame drift at the seam).
		const seamKeys = `expr:eq(n,0)+eq(n,${frames - 1})`;
		if (encode) {
			this.encoder = startFfmpeg(
				[
					"-hide_banner", "-loglevel", "error", "-y",
					"-f", "rawvideo", "-pix_fmt", "rgb24", "-s", SIZE, "-framerate", String(FPS), "-i", "pipe:0",
					// h264 master
					"-map", "0:v", "-an", "-vf", TO_BT709,
					"-c:v", "libx264", "-preset", "slow", "-crf", "18", "-profile:v", "high", "-pix_fmt", "yuv420p",
					// GOP pinned to the loop: x264's default keyint 250 put an I-frame inside a static hold
					// (output frame 250); only the forced seam keyframes 0 and N-1 remain.
					...COLOR_TAGS, "-g", String(frames), "-keyint_min", String(frames), "-sc_threshold", "0", "-force_key_frames", seamKeys, "-movflags", "+faststart", "-fps_mode", "cfr", "-r", String(FPS),
					mp4Path,
					// vp9
					"-map", "0:v", "-an", "-vf", TO_BT709,
					"-c:v", "libvpx-vp9", "-crf", "30", "-b:v", "0", "-row-mt", "1", "-deadline", "good", "-cpu-used", "2",
					"-pix_fmt", "yuv420p", ...COLOR_TAGS, "-g", String(frames), "-force_key_frames", seamKeys, "-fps_mode", "cfr", "-r", String(FPS),
					webmPath,
				],
				"h264+vp9",
			);
		}
		this.reader = this.#readLoop();
		this.reader.catch(() => {});
	}

	get ffmpegArgs() {
		return { downscale: this.downscaler.args, encode: this.encoder?.args ?? null };
	}

	async push(png) {
		await write(this.downscaler.child.stdin, png);
	}

	/** Output frame k from render frame k (see the loop design in contract section 1). */
	#compose(k, rgb) {
		const start = this.frames - this.dissolve;
		if (k < start) return rgb;
		const j = k - start + 1; // 1..D
		const head = this.head[k - start];
		const d = this.dissolve;
		const out = Buffer.allocUnsafe(FRAME_BYTES);
		const half = d >> 1;
		for (let p = 0; p < FRAME_BYTES; p++) {
			out[p] = ((rgb[p] * (d - j) + head[p] * j + half) / d) | 0;
		}
		return out;
	}

	async #readLoop() {
		const scratch = Buffer.allocUnsafe(FRAME_BYTES);
		let filled = 0;
		for await (const chunk of this.downscaler.child.stdout) {
			let offset = 0;
			while (offset < chunk.length) {
				const take = Math.min(FRAME_BYTES - filled, chunk.length - offset);
				chunk.copy(scratch, filled, offset, offset + take);
				filled += take;
				offset += take;
				if (filled === FRAME_BYTES) {
					filled = 0;
					const k = this.received++;
					const rgb = Buffer.from(scratch);
					if (k < this.dissolve) this.head.push(rgb);
					if (k === this.posterFrame) this.posterRgb = rgb;
					if (this.encoder) await write(this.encoder.child.stdin, this.#compose(k, rgb));
				}
			}
		}
		if (filled !== 0) throw new CaptureError(`downscaler produced a partial frame (${filled} bytes)`);
	}

	async finish() {
		this.downscaler.child.stdin.end();
		await this.downscaler.done();
		await this.reader;
		if (this.received !== this.frames) {
			throw new CaptureError(`downscaler returned ${this.received} frames, expected ${this.frames}`);
		}
		if (this.encoder) {
			this.encoder.child.stdin.end();
			await this.encoder.done();
		}
	}

	abort() {
		this.downscaler.kill();
		this.encoder?.kill();
	}
}

export async function encodePoster(rgb, posterPath) {
	const ff = startFfmpeg(
		[
			"-hide_banner", "-loglevel", "error", "-y",
			"-f", "rawvideo", "-pix_fmt", "rgb24", "-s", SIZE, "-i", "pipe:0",
			"-frames:v", "1", "-c:v", "libwebp", "-quality", "92", "-compression_level", "6", "-preset", "picture",
			posterPath,
		],
		"poster",
	);
	ff.child.stdin.end(rgb);
	await ff.done();
}

export function ffprobe(path) {
	const r = spawnSync(
		"ffprobe",
		[
			"-v", "error", "-count_frames", "-select_streams", "v:0",
			"-show_entries",
			"stream=codec_name,profile,width,height,pix_fmt,r_frame_rate,avg_frame_rate,nb_read_frames,color_space,color_transfer,color_primaries,color_range,duration:format=format_name,duration,size",
			"-of", "json", path,
		],
		{ encoding: "utf8", windowsHide: true },
	);
	if (r.status !== 0) throw new CaptureError(`ffprobe failed on ${path}: ${r.stderr}`);
	const json = JSON.parse(r.stdout);
	return { ...json.streams?.[0], format: json.format };
}

/** SSIM between decoded frames a and b of one file (used for the loop seam 479 -> 0). */
export function frameSsim(path, a, b) {
	const graph =
		`[0:v]split=2[s0][s1];` +
		`[s0]select='eq(n\\,${a})',setpts=N/FRAME_RATE/TB[x];` +
		`[s1]select='eq(n\\,${b})',setpts=N/FRAME_RATE/TB[y];` +
		`[x][y]ssim`;
	const r = spawnSync("ffmpeg", ["-hide_banner", "-nostats", "-i", path, "-filter_complex", graph, "-f", "null", "-"], {
		encoding: "utf8",
		windowsHide: true,
	});
	const m = r.stderr.match(/SSIM Y:([\d.]+).*?All:([\d.]+)/);
	if (!m) throw new CaptureError(`could not measure SSIM on ${path}: ${r.stderr.slice(-800)}`);
	return { y: Number(m[1]), all: Number(m[2]) };
}
