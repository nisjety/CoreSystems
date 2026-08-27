"""Media embedder — dense audio and video embeddings for the Data Plane v2 arms.

Closes the two modality gaps recorded in
`docs/core-research/embedding-modality-and-rag-audit-2026-08-19.md` §1: video and
audio were both "zero code". That audit also settled *which* models, and why the
answer is self-hosted rather than Azure:

  * Audio  — no Azure audio-similarity embedding product exists (Speaker
    Recognition is voice-print verification, not sound retrieval). The audit
    picks **LAION-CLAP** (193M params, sub-1GB, CPU-viable) for a simple start,
    with GLAP as the upgrade for unified speech+music+sound-event coverage.
  * Video  — Azure has no real fit either; Content Understanding's video analyzer
    is extraction/description, not a dense embedding model, and adopting it would
    mean re-architecting as "extract text, then text-embed" — a lossier and
    different capability. The audit picked **LanguageBind**; this service no
    longer uses it (see below).

Video tower: LanguageBind -> SigLIP 2 (default) or X-CLIP
--------------------------------------------------------
LanguageBind was the audit's pick and is gone. It could not be imported on any
current torch without TWO compatibility shims: it reaches
`torchvision.transforms.functional_tensor` through pytorchvideo (a module
torchvision removed in 0.17, via a package unmaintained since 2022), and it
vendors a transformers ~4.3x-era CLIP whose configs never set
`_attn_implementation`, so a forward pass died with `KeyError: None` on
transformers >= 4.48. Neither will be fixed upstream. The arm was consequently
opt-in, GPU-wanting, dark in every environment, and carried ~2 GB of wheels
(decord/av/pytorchvideo/opencv) to stay that way.

Two replacement towers now exist, both transformers-native, both fed by ffmpeg
frame sampling (ffmpeg is already required here for audio decode). Which one
loads is inferred from `MEDIA_VIDEO_MODEL`, never configured separately, so the
two cannot contradict each other:

  * **SigLIP 2** (`google/siglip2-*`, the DEFAULT) — frames encoded
    independently by a strong image tower, then mean-pooled. 768-dim at
    base/patch16-224. Cheap, CPU-viable, and the sharpest per-frame encoder of
    the two.
  * **X-CLIP** (`microsoft/xclip-*`) — indexing only, and NOT usable as a
    retrieval arm. Its text tower is conditioned on the candidate video
    (`prompts_generator`), so no precomputable query vector exists and
    `/embed/text` refuses on this backend. That makes it a cross-encoder —
    a possible future RERANKER over a candidate list, never a bi-encoder.
    512-dim. See the measurements below.

Both are CPU-viable — nothing here requires a GPU any more. Only SigLIP 2,
however, supports text->video retrieval: it is a true bi-encoder, so a query
vector can be precomputed and ANN-searched against stored video vectors. X-CLIP
cannot, which is why it is not the default.

### The temporal gap is still OPEN — do not assume otherwise

Neither tower can retrieve motion or direction. "Someone falling" and "someone
standing up" are the same video to both. X-CLIP was evaluated specifically as a
candidate fix and does NOT close it:

  * On two frame sequences that are exact reversals of each other (identical
    frame sets, opposite order), X-CLIP's video embedding is identical to six
    decimal places — cos = 1.000000, the same number plain mean pooling gives.
    Its Multiframe Integration Transformer pools order-invariantly in practice
    despite carrying a `position_embedding` parameter.
  * The same harness DOES separate different content (horizontal vs vertical
    motion, cos = 0.997), so the measurement is not a degenerate test.
  * End-to-end on real mp4s through this service — a box crossing left-to-right
    vs. the same clip reversed — SigLIP 2 gives cos 0.999118. Not the exact
    1.000000 of the frame-level test, because ffmpeg samples slightly different
    frames from each file, but nowhere near a usable margin.
  * On text->video retrieval via the bi-encoder path both towers pick the right
    caption, but SigLIP 2 separates far more sharply (0.182 vs 0.070/0.028) than
    X-CLIP (0.201 vs 0.171/0.185) — and X-CLIP's number is not even a fair
    comparison, for the architectural reason in the next section.

A TOWER SWAP WILL NOT CLOSE IT. On RTime-Binary — the published benchmark built
from exactly this reversal construction (ACM MM 2024) — CLIP scores 49.1%, UMT
49.8% and InternVideo2-1B 50.0% T2V. That is chance on a binary task, and
InternVideo2 is a video-native dual encoder trained on 50M video-text pairs. The
gap is where the field is, not a consequence of choosing frame pooling.

The cheapest real fix routes around vision: caption the video with a video-LLM
and index the caption into the existing text arms, where "the person falls"
becomes an ordinary text match. Full analysis, candidate table, hardware
constraints and the validation gate:
`docs/core-research/video-temporal-retrieval-gap-2026-08-25.md`.

### Two transformers 4.57 API defects, characterised

1. `XCLIPProcessor` silently IGNORES the `videos=` keyword — the argument that
   reads like the correct one — and returns an EMPTY batch. `images=frames` is
   what actually builds the video tensor, shape (1, num_frames, 3, H, W). The
   failure is downstream and confusing (a missing-argument error inside
   `get_video_features`), not a clear rejection at the processor.
2. `XCLIPModel.get_video_features` is BROKEN: it annotates `self.mit(...)` as
   `BaseModelOutputWithPooling` and reads `.pooler_output`, but the call returns
   a plain 2-tuple, so it raises AttributeError. `return_dict=True` does not
   help. The upstream one-line fix is `mit_outputs[1]`.

   This service does not monkeypatch it. The full `model(...)` forward computes
   the same value and normalises it, and `cosine(normalize(mit[1]),
   forward.video_embeds)` measures 1.00000000 — so the public-API path is
   verified equivalent to a fixed helper, touches no internals, and needs no
   change when upstream is repaired.

The tower determines the embedding SPACE and the width, so switching always
means a new Qdrant collection (Qdrant fixes vector size per collection; a swap
is never a resize). `QDRANT_VIDEO_COLLECTION` must move with
`MEDIA_VIDEO_MODEL`. `/healthz` reports `backend`, `dim`, `frames` and
`order_sensitive` so a deployment's actual capability is observable rather than
inferred from a model name.

The audio tower and the SigLIP 2 video tower are cross-modal bi-encoders: text
and media land in ONE space, so a text query retrieves media directly. That is
what makes these real retrieval arms rather than similarity-only classifiers,
and it mirrors how Embed v4 already serves the visual arm (one space for the
query text and the page image).

Contract (mirrors services/colqwen-reranker's host-independent shape):
    POST /embed/audio  { "urls": [str, ...] }             -> { "embeddings": [[f32]], "dim": int }
    POST /embed/video  { "urls": [str, ...] }             -> { "embeddings": [[f32]], "dim": int }
    POST /embed/text   { "texts": [str], "space": "audio" } -> { "embeddings": [[f32]], "dim": int }
    POST /filmstrip/video { "urls": [str, ...] }
        -> { "filmstrips": [{ "image_base64": str, "mime_type": str,
                              "frames": int, "seconds": [f], "cols": int }] }
       Time-ordered frames tiled into one labelled JPEG, for caption-to-text.
       Needs ffmpeg + Pillow only — NOT the video tower, so it works with
       MEDIA_VIDEO_ENABLED=false.
    GET  /healthz -> { "ok": true, "audio": {...}, "video": {...} }

Vectors are L2-normalised, so Qdrant Cosine and dot product agree and the RRF
fusion in retrieval-engine sees comparable score scales across arms.
"""

import base64
import io
import os
import subprocess
import tempfile

import httpx
import numpy as np
import torch
from fastapi import FastAPI, HTTPException
from PIL import Image, ImageDraw, ImageFont
from pydantic import BaseModel

# ── Device / precision ────────────────────────────────────────────────────────
# Both towers are CPU-viable: CLAP by the audit's design, and SigLIP 2 base
# because eight frames per video is a small batch. Neither arm requires a GPU.
if torch.cuda.is_available():
    DEVICE, DTYPE = "cuda", torch.float16
elif getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
    DEVICE, DTYPE = "mps", torch.float16
else:
    DEVICE, DTYPE = "cpu", torch.float32

AUDIO_MODEL_ID = os.environ.get("MEDIA_AUDIO_MODEL", "laion/clap-htsat-unfused")
# Two video towers, both native to transformers. Which one loads is INFERRED
# from the model id rather than configured separately — a second knob could
# contradict the first, and the failure would be a silently wrong embedding
# space rather than an error.
#
#   * SigLIP 2 (DEFAULT, `google/siglip2-*`) — frames encoded by an image tower
#     and mean-pooled. 768-dim at base/patch16-224. Cheap and the sharper
#     per-frame encoder.
#   * X-CLIP (`microsoft/xclip-*`) — EXPERIMENTAL video-pretrained dual encoder,
#     512-dim. NOT a temporal upgrade: measured on an exact frame reversal its
#     embedding is unchanged (cos=1.000000), identical to mean pooling. See the
#     module docstring for the full measurement.
#
# NEITHER tower is order-sensitive, so motion and direction are not retrievable
# on this arm at all. Do not read the choice here as a fix for that.
#
# Changing this changes the embedding SPACE and usually the width, so it always
# means a new Qdrant collection — set QDRANT_VIDEO_COLLECTION with it.
VIDEO_MODEL_ID = os.environ.get("MEDIA_VIDEO_MODEL", "google/siglip2-base-patch16-224")
VIDEO_BACKEND = "siglip2" if "siglip" in VIDEO_MODEL_ID.lower() else "xclip"
# Still opt-in, but for a different and much smaller reason than before: the
# tower is another ~1 GB of weights to download and hold resident, and a
# deployment with no video corpus should not pay for it. It no longer pulls
# decord/av/pytorchvideo, and no longer wants a GPU.
VIDEO_ENABLED = os.environ.get("MEDIA_VIDEO_ENABLED", "false").strip().lower() in (
    "1",
    "true",
    "yes",
)
PORT = int(os.environ.get("PORT", "8095"))

# CLAP is trained at 48 kHz mono; feeding it anything else silently degrades the
# embedding rather than erroring, so resampling is not optional.
AUDIO_SR = 48_000
# Seconds of audio per embedding. The producer is expected to segment long media
# upstream (one event per segment, like one event per page image); this cap only
# bounds a single request so a stray hour-long file cannot exhaust memory.
AUDIO_MAX_SECONDS = int(os.environ.get("MEDIA_AUDIO_MAX_SECONDS", "60"))
# VIDEO_FRAMES is defined with the video tower below, because X-CLIP pins it to
# the checkpoint's trained frame count rather than letting the env decide.

_HTTP = httpx.Client(timeout=60.0, follow_redirects=True)

# Optional host rewrite, same rationale as colqwen-reranker: the producer emits
# Docker-internal URLs that a server outside that network cannot resolve.
_URL_REWRITES = [
    tuple(p.split("=", 1))
    for p in os.environ.get("MEDIA_URL_REWRITE", "").split(",")
    if "=" in p
]


def _fetch(url: str) -> bytes:
    for frm, to in _URL_REWRITES:
        url = url.replace(frm, to)
    r = _HTTP.get(url)
    r.raise_for_status()
    return r.content


# ── Audio: LAION-CLAP ─────────────────────────────────────────────────────────
print(f"[media] loading audio tower {AUDIO_MODEL_ID} on {DEVICE}", flush=True)
from transformers import ClapModel, ClapProcessor

_audio_model = ClapModel.from_pretrained(AUDIO_MODEL_ID).to(DEVICE).eval()
_audio_proc = ClapProcessor.from_pretrained(AUDIO_MODEL_ID)
AUDIO_DIM = int(_audio_model.config.projection_dim)
print(f"[media] audio ready (dim={AUDIO_DIM})", flush=True)

# ── Video: SigLIP 2 frame pooling (default) or X-CLIP ────────────────────────
# No compatibility shims on either path. Both of the ones this replaced are
# described in the module docstring; the point of the swap is that neither is
# needed. Both towers are transformers-native, so image/video and text sides are
# one model and one processor, and the shared space is the model's own.
_video_model = None
_video_proc = None
VIDEO_DIM = 0
# Frames per video. X-CLIP is TRAINED at a fixed frame count (8 for
# base-patch32) and its multiframe module is shaped by it, so the value is read
# off the checkpoint rather than configured — feeding a different count is a
# shape error at best and a silently degraded embedding at worst. SigLIP 2 pools
# whatever it is given, so there MEDIA_VIDEO_FRAMES is a real knob.
VIDEO_FRAMES = int(os.environ.get("MEDIA_VIDEO_FRAMES", "8"))
# Placeholder text for X-CLIP's full forward (see `embed_video`). Its output is
# discarded; only `video_embeds` is used. Kept as a constant so it is obviously
# not caller data.
_XCLIP_FORWARD_DUMMY_TEXT = ["video"]

# Filmstrip layout for caption-to-text. Frames are tiled in reading order so a
# vision model reads them as a sequence. 4 columns keeps an 8-frame strip at
# 2 rows, which stays well inside a single vision request without downscaling
# the tiles into uselessness.
FILMSTRIP_COLS = int(os.environ.get("MEDIA_FILMSTRIP_COLS", "4"))
FILMSTRIP_CELL = int(os.environ.get("MEDIA_FILMSTRIP_CELL", "224"))
FILMSTRIP_QUALITY = int(os.environ.get("MEDIA_FILMSTRIP_QUALITY", "85"))
if VIDEO_ENABLED:
    print(
        f"[media] loading video tower {VIDEO_MODEL_ID} (backend={VIDEO_BACKEND}) on {DEVICE}",
        flush=True,
    )
    from transformers import AutoModel, AutoProcessor

    _video_model = AutoModel.from_pretrained(VIDEO_MODEL_ID).to(DEVICE).eval()
    _video_proc = AutoProcessor.from_pretrained(VIDEO_MODEL_ID)

    if VIDEO_BACKEND == "xclip":
        # Non-negotiable for X-CLIP: its multiframe module is built for exactly
        # this many frames. Override the env value rather than honouring it.
        _native_frames = int(
            getattr(getattr(_video_model.config, "vision_config", None), "num_frames", 0) or 0
        )
        if _native_frames > 0 and _native_frames != VIDEO_FRAMES:
            print(
                f"[media] MEDIA_VIDEO_FRAMES={VIDEO_FRAMES} ignored; {VIDEO_MODEL_ID} "
                f"is trained at {_native_frames} frames and its multiframe module "
                f"requires exactly that",
                flush=True,
            )
            VIDEO_FRAMES = _native_frames

    # Read the projection width off the loaded config rather than trusting an
    # env var: the dimension is a property of the checkpoint, and a mismatch
    # against the Qdrant collection fails every upsert. MEDIA_VIDEO_DIM stays
    # available only as an override for a checkpoint whose config does not
    # expose the field.
    _configured_dim = os.environ.get("MEDIA_VIDEO_DIM", "").strip()
    if _configured_dim:
        VIDEO_DIM = int(_configured_dim)
    else:
        _cfg = _video_model.config
        # X-CLIP is CLIP-shaped and exposes `projection_dim`; SigLIP 2 projects
        # to its text tower's hidden size. Try both before giving up, and give
        # up loudly rather than defaulting to a plausible-looking number.
        VIDEO_DIM = int(
            getattr(_cfg, "projection_dim", 0)
            or getattr(getattr(_cfg, "text_config", None), "hidden_size", 0)
            or 0
        )
        if VIDEO_DIM <= 0:
            raise RuntimeError(
                f"cannot determine embedding width for {VIDEO_MODEL_ID}; "
                "set MEDIA_VIDEO_DIM explicitly"
            )
    print(
        f"[media] video ready (model={VIDEO_MODEL_ID} backend={VIDEO_BACKEND} "
        f"dim={VIDEO_DIM} frames={VIDEO_FRAMES} order_sensitive=no)",
        flush=True,
    )
else:
    print("[media] video tower disabled (MEDIA_VIDEO_ENABLED=false)", flush=True)


def _probe_duration_seconds(path: str) -> float:
    """Media duration via ffprobe, or 0.0 when it cannot be determined.

    0.0 is a real outcome, not an error: streamed or truncated containers often
    carry no duration metadata, and `_sample_frames` has a metadata-free
    fallback for exactly that case.
    """
    proc = subprocess.run(
        [
            "ffprobe", "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            path,
        ],
        capture_output=True,
        check=False,
    )
    try:
        return max(0.0, float(proc.stdout.decode().strip()))
    except (ValueError, UnicodeDecodeError):
        return 0.0


def _sample_frames(path: str, want: int) -> list["Image.Image"]:
    """Up to `want` frames spread across the video, as PIL images.

    Two strategies, because container metadata is not dependable:

    * Known duration -> an `fps` filter of want/duration, which spaces frames
      evenly across the whole video. Even spacing is what makes the mean-pooled
      vector represent the video rather than its opening seconds.
    * Unknown duration -> ffmpeg's `thumbnail` filter, which picks the most
      representative frame out of each window without needing to know how long
      the file is. Less even, but never biased to the start.

    PNG rather than JPEG: these frames go straight into a vision encoder, and
    JPEG artefacts are avoidable noise on the way in.
    """
    duration = _probe_duration_seconds(path)
    if duration > 0:
        # Guard the divide and keep the rate sane for very short clips.
        rate = max(want / duration, 0.001)
        video_filter = f"fps={rate:.6f}"
    else:
        video_filter = f"thumbnail={max(want, 1)}"

    proc = subprocess.run(
        [
            "ffmpeg", "-nostdin", "-loglevel", "error",
            "-i", path,
            "-vf", video_filter,
            "-frames:v", str(max(want, 1)),
            "-f", "image2pipe", "-vcodec", "png",
            "pipe:1",
        ],
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0 or not proc.stdout:
        raise HTTPException(
            422, f"video decode failed: {proc.stderr.decode()[:200]}"
        )

    # image2pipe concatenates whole PNGs; split on the 8-byte signature to
    # recover them individually rather than asking ffmpeg for numbered temp
    # files (which would need a writable scratch dir per request).
    blob = proc.stdout
    signature = b"\x89PNG\r\n\x1a\n"
    offsets = []
    start = blob.find(signature)
    while start != -1:
        offsets.append(start)
        start = blob.find(signature, start + len(signature))
    frames: list[Image.Image] = []
    for index, begin in enumerate(offsets):
        end = offsets[index + 1] if index + 1 < len(offsets) else len(blob)
        frames.append(Image.open(io.BytesIO(blob[begin:end])).convert("RGB"))
    if not frames:
        raise HTTPException(422, "video produced no decodable frames")
    return frames


def _build_filmstrip(frames: list["Image.Image"], seconds: list[float]) -> bytes:
    """Time-ordered frames tiled into ONE labelled image, as JPEG bytes.

    This is the input to caption-to-text (see
    `docs/core-research/video-temporal-retrieval-gap-2026-08-25.md`): a vision
    model describes the filmstrip, and because the tiles are laid out in
    reading order and stamped with their timestamps, the description it produces
    is a NARRATIVE — "a person walks to the door, then falls" — rather than a
    bag of frame contents. That narrative is what makes motion retrievable,
    because word order in text is something the text arms genuinely model.

    One image rather than N separate frames deliberately: it costs one vision
    call instead of N, and it lets the model see the progression rather than
    describing each frame in isolation with no idea what came before.

    Labels are drawn with PIL's built-in bitmap font — no font file to ship, and
    the digits only have to be legible to the model, not pretty.
    """
    if not frames:
        raise HTTPException(422, "no frames to build a filmstrip from")

    cols = min(FILMSTRIP_COLS, len(frames))
    rows = (len(frames) + cols - 1) // cols
    label_h = 14
    cell_w, cell_h = FILMSTRIP_CELL, FILMSTRIP_CELL
    sheet = Image.new("RGB", (cols * cell_w, rows * (cell_h + label_h)), (0, 0, 0))
    draw = ImageDraw.Draw(sheet)
    try:
        font = ImageFont.load_default()
    except Exception:  # pragma: no cover - Pillow always ships one
        font = None

    for index, frame in enumerate(frames):
        col, row = index % cols, index // cols
        x, y = col * cell_w, row * (cell_h + label_h)
        # Timestamps are what let the model say "then" instead of "and also".
        stamp = f"{index + 1}  t={seconds[index]:.1f}s" if index < len(seconds) else f"{index + 1}"
        draw.text((x + 2, y + 2), stamp, fill=(255, 255, 0), font=font)
        sheet.paste(frame.resize((cell_w, cell_h)), (x, y + label_h))

    buf = io.BytesIO()
    sheet.save(buf, format="JPEG", quality=FILMSTRIP_QUALITY)
    return buf.getvalue()


def _frame_seconds(path: str, count: int) -> list[float]:
    """Approximate timestamp per sampled frame, evenly spread over the duration.

    Approximate on purpose: `_sample_frames` asks ffmpeg for an even spread and
    ffmpeg lands on real frame boundaries, so these are labels for the model to
    reason about ordering with, not a citation-grade index. Zero-duration
    (unprobeable) media yields a plain 0.0 for every frame, which still leaves
    the tiles in order.
    """
    duration = _probe_duration_seconds(path)
    if duration <= 0 or count <= 1:
        return [0.0] * count
    step = duration / count
    return [round(i * step, 2) for i in range(count)]


def _pool_frames(frame_features: torch.Tensor) -> torch.Tensor:
    """Frame vectors -> one video vector.

    Normalise, mean, normalise again. The first normalisation matters: without
    it a frame the encoder happened to give a larger magnitude would dominate
    the average, so the video vector would track encoder confidence instead of
    content. The second puts the result back on the unit sphere so it is
    comparable with the text vectors and with every other arm's scores.
    """
    normalised = torch.nn.functional.normalize(frame_features.float(), p=2, dim=-1)
    return normalised.mean(dim=0, keepdim=True)


def _decode_audio(raw: bytes) -> np.ndarray:
    """Any container/codec -> mono float32 at 48 kHz, via ffmpeg.

    ffmpeg rather than librosa/soundfile because the producer's media is
    arbitrary (mp3/m4a/opus/webm, and the audio track of an mp4); a pure-Python
    decoder would need a codec matrix we would then have to maintain.
    """
    proc = subprocess.run(
        [
            "ffmpeg", "-nostdin", "-loglevel", "error",
            "-i", "pipe:0",
            "-t", str(AUDIO_MAX_SECONDS),
            "-ac", "1", "-ar", str(AUDIO_SR),
            "-f", "f32le", "pipe:1",
        ],
        input=raw,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0 or not proc.stdout:
        raise HTTPException(422, f"audio decode failed: {proc.stderr.decode()[:200]}")
    return np.frombuffer(proc.stdout, dtype=np.float32)


def _l2(t: torch.Tensor) -> list[list[float]]:
    t = torch.nn.functional.normalize(t.float(), p=2, dim=-1)
    return t.cpu().tolist()


class UrlsRequest(BaseModel):
    urls: list[str]


class TextRequest(BaseModel):
    texts: list[str]
    # Which shared space to project into. A text query must be embedded with the
    # SAME tower as the media it is being compared against — CLAP text and
    # SigLIP 2 text are different spaces and mixing them is meaningless.
    space: str = "audio"


app = FastAPI(title="media-embedder")


@app.get("/healthz")
def healthz():
    return {
        "ok": True,
        "device": DEVICE,
        "audio": {"model": AUDIO_MODEL_ID, "dim": AUDIO_DIM, "enabled": True},
        "video": {
            "model": VIDEO_MODEL_ID if VIDEO_ENABLED else None,
            "dim": VIDEO_DIM,
            "enabled": VIDEO_ENABLED,
            "backend": VIDEO_BACKEND if VIDEO_ENABLED else None,
            # Reported so a caller diagnosing a bad video result does not have to
            # guess. False for BOTH towers, and measured rather than assumed:
            # on an exact frame reversal each returns an unchanged embedding
            # (see the module docstring). Motion and direction are therefore not
            # retrievable on this arm; only what appears on screen is.
            "order_sensitive": False if VIDEO_ENABLED else None,
            "frames": VIDEO_FRAMES if VIDEO_ENABLED else None,
        },
    }


@app.post("/embed/audio")
def embed_audio(req: UrlsRequest):
    if not req.urls:
        return {"embeddings": [], "dim": AUDIO_DIM}
    waves = [_decode_audio(_fetch(u)) for u in req.urls]
    inputs = _audio_proc(audios=waves, sampling_rate=AUDIO_SR, return_tensors="pt")
    inputs = {k: v.to(DEVICE) for k, v in inputs.items()}
    with torch.no_grad():
        feats = _audio_model.get_audio_features(**inputs)
    return {"embeddings": _l2(feats), "dim": AUDIO_DIM}


@app.post("/embed/video")
def embed_video(req: UrlsRequest):
    if not VIDEO_ENABLED:
        raise HTTPException(503, "video tower disabled; set MEDIA_VIDEO_ENABLED=true")
    if not req.urls:
        return {"embeddings": [], "dim": VIDEO_DIM}
    paths = []
    try:
        # ffmpeg/ffprobe seek within a file; neither works on a pipe for the
        # duration probe, so the fetched bytes are spilled to disk first.
        for u in req.urls:
            fh = tempfile.NamedTemporaryFile(suffix=".mp4", delete=False)
            fh.write(_fetch(u))
            fh.close()
            paths.append(fh.name)

        # One vector per video. Videos are processed one at a time rather than as
        # one big batch so a long clip cannot push a short one's frames out of a
        # shared budget, and so it is unambiguous which frames belong to which
        # video.
        vectors = []
        for path in paths:
            frames = _sample_frames(path, VIDEO_FRAMES)
            if VIDEO_BACKEND == "xclip":
                # The X-CLIP path. `images=frames` is correct and NOT a
                # fallback to the image tower: XCLIPProcessor interprets a flat
                # list of frames as ONE video and emits
                # `pixel_values` of shape (1, num_frames, 3, H, W) — verified
                # against transformers 4.57. The `videos=` keyword, which reads
                # like the right one, is silently IGNORED there and yields an
                # empty batch, so `get_video_features` fails on a missing
                # argument rather than quietly degrading.
                #
                # X-CLIP needs exactly its trained frame count, because the
                # multiframe module's structure is sized for it. Short
                # or unseekable clips can yield fewer, so pad by repeating the
                # last frame: a held final frame reads as "nothing further
                # happens", whereas a short sequence is a shape error.
                if len(frames) < VIDEO_FRAMES:
                    frames = frames + [frames[-1]] * (VIDEO_FRAMES - len(frames))
                # The full forward, NOT `get_video_features`.
                #
                # THE UPSTREAM BUG (transformers 4.57, modeling_x_clip.py):
                # `get_video_features` ends with
                #     mit_outputs: BaseModelOutputWithPooling = self.mit(cls_features)
                #     video_embeds = mit_outputs.pooler_output
                # but `self.mit(...)` returns a plain 2-tuple, so this raises
                # `AttributeError: 'tuple' object has no attribute
                # 'pooler_output'`. The type annotation is simply wrong; the
                # one-line upstream fix is `mit_outputs[1]`. Passing
                # `return_dict=True` does NOT help (verified).
                #
                # THE CORRECT VALUE, verified rather than assumed: the forward
                # computes `video_embeds = mit_outputs[1]` and then L2-normalises
                # it, and `cosine(normalize(mit[1]), forward.video_embeds)`
                # measures 1.00000000. So this path returns exactly what a fixed
                # `get_video_features` would, using only public API — no
                # monkeypatch, no reaching into `.mit`/`.vision_model`, and it
                # keeps working unchanged once upstream is repaired.
                #
                # The forward computes both towers, so it needs text. One fixed
                # token is passed and its output discarded: a short string
                # through a CLIP text encoder is negligible beside 8 frames
                # through a ViT.
                inputs = _video_proc(
                    images=frames,
                    text=_XCLIP_FORWARD_DUMMY_TEXT,
                    return_tensors="pt",
                    padding=True,
                )
                inputs = {k: v.to(DEVICE) for k, v in inputs.items()}
                with torch.no_grad():
                    features = _video_model(**inputs).video_embeds
                vectors.append(features.float())
            else:
                inputs = _video_proc(images=frames, return_tensors="pt")
                inputs = {k: v.to(DEVICE) for k, v in inputs.items()}
                with torch.no_grad():
                    frame_features = _video_model.get_image_features(**inputs)
                vectors.append(_pool_frames(frame_features))
        return {"embeddings": _l2(torch.cat(vectors, dim=0)), "dim": VIDEO_DIM}
    finally:
        for p in paths:
            try:
                os.unlink(p)
            except OSError:
                pass


@app.post("/filmstrip/video")
def filmstrip_video(req: UrlsRequest):
    """Time-ordered frames of each video as ONE labelled JPEG (base64).

    Serves caption-to-text: the caller sends this image to a vision model and
    indexes the resulting narrative as TEXT, which is how motion and ordering
    become retrievable at all — see
    `docs/core-research/video-temporal-retrieval-gap-2026-08-25.md` for why no
    video *embedding* tower achieves that.

    Deliberately NOT gated on MEDIA_VIDEO_ENABLED: this path needs ffmpeg and
    Pillow only, never the video tower's weights. A deployment can caption
    video without paying for an embedding model it does not want.
    """
    if not req.urls:
        return {"filmstrips": []}
    out = []
    paths = []
    try:
        for u in req.urls:
            fh = tempfile.NamedTemporaryFile(suffix=".mp4", delete=False)
            fh.write(_fetch(u))
            fh.close()
            paths.append(fh.name)
        for path in paths:
            frames = _sample_frames(path, VIDEO_FRAMES)
            seconds = _frame_seconds(path, len(frames))
            jpeg = _build_filmstrip(frames, seconds)
            out.append(
                {
                    "image_base64": base64.b64encode(jpeg).decode("ascii"),
                    "mime_type": "image/jpeg",
                    "frames": len(frames),
                    "seconds": seconds,
                    "cols": min(FILMSTRIP_COLS, len(frames)),
                }
            )
        return {"filmstrips": out}
    finally:
        for p in paths:
            try:
                os.unlink(p)
            except OSError:
                pass


@app.post("/embed/text")
def embed_text(req: TextRequest):
    """Embed a text query into the audio or video space (cross-modal retrieval)."""
    if not req.texts:
        return {"embeddings": [], "dim": 0}
    space = req.space.strip().lower()
    if space == "audio":
        inputs = _audio_proc(text=req.texts, return_tensors="pt", padding=True)
        inputs = {k: v.to(DEVICE) for k, v in inputs.items()}
        with torch.no_grad():
            feats = _audio_model.get_text_features(**inputs)
        return {"embeddings": _l2(feats), "dim": AUDIO_DIM}
    if space == "video":
        if not VIDEO_ENABLED:
            raise HTTPException(503, "video tower disabled; set MEDIA_VIDEO_ENABLED=true")
        if VIDEO_BACKEND == "xclip":
            # FAIL CLOSED. X-CLIP has no video-independent text embedding, so
            # there is no correct value to return here.
            #
            # Its forward computes
            #     text_embeds = text_embeds + self.prompts_generator(text_embeds, img_features)
            # where `img_features` comes from the CANDIDATE VIDEO. The text
            # vector the model actually matches a video against is therefore a
            # function of that video — measured: the same query embedded against
            # two different videos gives cos 0.998311, not 1.0.
            #
            # `get_text_features` returns the UN-prompted text vector, which is
            # cos 0.9439 away from the prompted one the model was trained to
            # align. Returning it would look like it worked and quietly degrade
            # every video query, which is worse than refusing.
            #
            # This makes X-CLIP a cross-encoder: usable for RERANKING a
            # candidate list (score each (query, video) pair through the
            # forward), never as a bi-encoder arm where one query vector is
            # ANN-searched against precomputed video vectors. Use the SigLIP 2
            # backend for the retrieval arm.
            raise HTTPException(
                501,
                "X-CLIP has no video-independent text embedding: its text tower is "
                "conditioned on the candidate video (prompts_generator), so a "
                "precomputable query vector does not exist. It is a reranker, not a "
                "bi-encoder. Set MEDIA_VIDEO_MODEL to a siglip2 checkpoint for "
                "text->video retrieval.",
            )
        with torch.no_grad():
            # Padding differs by tower and is not cosmetic. SigLIP/SigLIP 2 are
            # trained with every sequence padded to the full 64-token context,
            # so dynamic padding shifts the embedding enough to measurably hurt
            # retrieval. X-CLIP is CLIP-shaped and expects ordinary dynamic
            # padding, where forcing max_length would instead pad to 77 tokens
            # of noise.
            padding = "max_length" if VIDEO_BACKEND == "siglip2" else True
            batch = _video_proc(
                text=req.texts,
                return_tensors="pt",
                padding=padding,
                truncation=True,
            )
            batch = {k: v.to(DEVICE) for k, v in batch.items()}
            feats = _video_model.get_text_features(**batch)
        return {"embeddings": _l2(feats), "dim": VIDEO_DIM}
    raise HTTPException(422, f"space must be 'audio' or 'video', got {req.space!r}")


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="0.0.0.0", port=PORT)
