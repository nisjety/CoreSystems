# Scene capture harness

Deterministic 60 fps capture of the Verevon product scenes (`02-plan`, `03-undersok`), per
`plans/product-scenes/CONTRACT.md` section 4. Entry point: `scripts/capture-scene.mjs`.

```sh
# from apps/verevon-web
node scripts/capture-scene.mjs --scene 02-plan                         # starts the SPA dev server on :5216 itself
node scripts/capture-scene.mjs --scene 02-plan --url http://127.0.0.1:5211   # use a running scene-mode server
node scripts/capture-scene.mjs --scene 02-plan --publish               # copy media to public/product-scenes/
node scripts/capture-scene.mjs --help
```

Options: `--out <dir>` (default `%TEMP%/verevon-scene-capture/<id>`, never inside the repo),
`--frames` (default `durationMs * 60 / 1000` = 480), `--poster-frame` (default 420),
`--viewport WxH` (16:10 only; default is the page's `__verevonScene.viewport`), `--keep-frames`,
`--no-encode`, `--no-warmup`, `--no-caret-reset`, `--port`, `--frame-timeout`, `--ready-timeout`, `--quiet`.
Exit codes: 0 ok, 1 capture or output check failed, 2 usage error.

Without `--url` the harness runs `pnpm exec vite --host 127.0.0.1 --port <port> --strictPort` in the
SPA with `VITE_SCENE_MODE=1`. It stops the whole process tree afterwards, and also on failure,
Ctrl+C or exit, and then checks that every pid is gone.

## Outputs (in `--out`)

| File | Content |
|---|---|
| `<id>.mp4` | h264 high, crf 18, yuv420p, bt709 tagged, faststart, 1280x800, 60 fps CFR, 480 frames |
| `<id>.webm` | vp9, crf 30, row-mt, yuv420p, bt709, 60 fps CFR |
| `<id>-poster.webp` | render frame 420 (t = 7.0 s), 1280x800 |
| `samples/frame-NNNN.png`, `samples/text-NNNN.txt` | every 30th raw 2560x1600 frame and its `document.body.innerText` |
| `frame-hashes.json` | sha256 of every raw PNG |
| `capture-report.json` | mechanism, exact Chromium command line, clock alignment, per-frame pointer and wall times, scene log, unmatched `/api` calls, warnings, ffprobe, seam SSIM, cleanup proof |
| `frames/frame-NNNN.png` | only with `--keep-frames` |

Pipeline: the raw PNGs (2560x1600 at device scale factor 2560/W) are piped into ffmpeg, scaled with
lanczos to 1280x800 rgb24 and read back by node. Node composes the loop:
`out[k] = render[k]` for k < 450, and `out[k] = round(((30-j)*render[k] + j*render[k-450]) / 30)` with
`j = k-449` (1..30). So output 479 equals render 29, which equals the static render 0. Frames 0 and
479 are forced keyframes, so they also encode almost identically. Nothing large is written to disk.

## Mechanism (measured on this Windows machine, Chromium 153.0.8010.12)

Chosen: **chrome-headless-shell with `--deterministic-mode` (BeginFrame control) plus CDP virtual time.**

- (a) Virtual time plus `Page.captureScreenshot` alone was **rejected**. Timers and `performance.now()`
  followed virtual time exactly, but `document.timeline` followed the real clock. A 300 ms
  transition finished in about 6 frames instead of 18.
- (b) `HeadlessExperimental.beginFrame` works on Windows in the headless shell, so it **was chosen**.
  The harness supplies every frame time, so CSS transitions, CSS/Web animations and rAF advance exactly
  per frame, and the screenshot is taken after all compositor stages ran. The device scale factor must
  be real (`--force-device-scale-factor` and `--window-size`); beginFrame screenshots ignore emulated
  DSF and came back 1120x700.
- (c) Seeking `document.getAnimations()` plus a timer shim was **not needed**. It would also miss
  rAF, smooth scrolling and transitions started between frames, and it changes page code.

The clock rules that make it bit-exact. Each rule fixes a measured run-to-run difference:

1. **100 µs grid anchored at navigation start.** Blink coarsens animation time to 100 µs with a
   random per-process jitter. Frame times are placed exactly on that grid, anchored at
   `Performance.getMetrics` NavigationStart.
2. **The animation clock never lags virtual time.** Each frame is rendered with the next frame's
   time, and a no-op rAF loop forces a main frame on every BeginFrame. Otherwise a style update forced
   between frames uses Blink's "estimated next frame" time, which is off the grid.
3. **Microsecond-exact conversions.** Every value sent over CDP carries a 0.1 µs margin.
4. **Constant offset from navigation start.** Scene t = 0 is always 5.000000 s after navigation
   start. Without it, frames that fall exactly on a `steps()` boundary flipped through floating-point
   rounding.
5. **Pointer and caret handling.**
   - Mouse events: `Input.dispatchMouseEvent` only acks after a main frame. The events are sent
     unawaited, then 1-3 no-display "input flush" BeginFrames run before the screenshot frame.
   - Caret: a focused field's caret blink is restarted at t = 0 (blur, one frame, focus, selection
     restored).

Frame i (t_i = i·1000/60 ms) shows every timer due at or before t_i + 0.5 ms. CSS animation progress
and rAF timestamps in frame i correspond to t_(i+1), because the animation clock leads by one frame.

## Rules for scene authors

- Schedule with `setTimeout` relative to `start()` or the intercepted POST. Never compute delays
  from `performance.now()` or `Date.now()` differences: Chromium's clamp makes `performance.now()`
  read up to ±0.1 ms off at identical virtual times, and the error differs per run.
- `Date` before `start()` is the real wall clock. After the pause it is 2026-10-09 12:00:00 Oslo plus
  the 0.5-5.5 s alignment step, which varies per run. The minute is stable; seconds are not. If any
  label depends on Date (greetings, "for X s"), install a fixed Date in the runtime before the app
  renders.
- Nothing may animate while at rest at `ready`: infinite animations, spinners or a shimmer have a
  real-time phase. The report warns (`runningAnimationsAtReady`).
- The resting state must survive about 5 s of idle virtual time before t = 0 (rule 4): pending app
  timers fire then.
- Preload lazy JS chunks before `ready`. A fetch pauses virtual time, but compiling and evaluating the
  module does not. The report lists `timelineRequests` and warns about scripts.
- Avoid off-main-thread async during the timeline (`img.decode()`, `createImageBitmap`,
  `crypto.subtle`, workers, IndexedDB, Blob reads). They complete in real time.
- Use rAF timestamps, not rAF counts: rAF also runs in input-flush frames while the pointer moves.
- Scroll-driven animations (`scroll-timeline`) are untested; avoid them. Programmatic smooth
  scrolling is tested and deterministic.
- The cursor overlay must be `pointer-events: none`. Aim the pointer at element centres: Chromium
  hit-tests up to about 0.7 CSS px outside an element's edge.
- The viewport must be exactly 16:10. If the page reports a different viewport than the launch
  guess, the harness relaunches with the page's value.

## Cost

About 0.11-0.21 s per frame (synthetic page 0.11, SPA 02-plan 0.20), so 480 frames take 75-125 s
wall time including warm-up, encode, probes and teardown. Chrome's own exit can take 20-110 s on a
loaded machine, so the harness force-kills its processes after 5 s.
