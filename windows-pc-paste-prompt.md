# Paste-prompt for the Windows PC session

Copy everything in the fenced block below into a fresh Claude Code session on the Windows PC,
after you've pulled the `model-plane-harness` branch.

```
I'm continuing GPU-dependent work on this machine (Windows 11, Intel Core i7-13700H, 32 GB RAM,
NVIDIA RTX A1000 Laptop GPU with 6 GB VRAM, 1 TB SSD) that a Mac without a discrete GPU couldn't
run or test. The repo is CoreSystem, a multi-plane monorepo — read
`CLAUDE.md` at the repo root first for orientation, then read
"data plane kontinuati from windows.md" at the repo root in full before doing anything else —
it has the real, verified current state of both workstreams below, with exact file paths and the
specific VRAM math for this exact GPU. Don't skim it — the two things you're about to try both
have a real chance of simply not fitting in 6 GB at their currently-pinned precision, and the doc
explains exactly why and what to do about it.

First, sanity-check the environment before touching anything:
1. Confirm you're on `model-plane-harness` and have commit `1ec4639e` ("feat(inference-core): wire
   Cohere Command A Plus via Azure AI Foundry") — if not, fetch/pull first.
2. Confirm `nvidia-smi` shows the RTX A1000 with ~6 GB VRAM, so we're working from real numbers,
   not assumed ones.

Then, in order:

1. **ColQwen2 visual reranker** (`apps/Data Plane v2/services/colqwen-reranker/app.py`) — try
   loading `vidore/colqwen2.5-v0.2` for real on this GPU and see whether it fits. The doc's math
   says it probably won't (needs ~7.5 GB in fp16, this card has 6 GB) — confirm that live rather
   than assuming, then work out a real fix (quantization, or a smaller checkpoint) rather than
   giving up at the first OOM.

2. **Local Llama reasoning provider** (Model Plane, decision D-C in the sovereign-rag docs) — this
   is greenfield, no code exists yet. The plan names "Llama 3.3," which is only released as 70B and
   cannot fit this card at any quantization. Before writing any provider code, help me decide: do we
   (a) build/test the provider mechanism here against a small substitute model (Llama 3.2 3B or
   3.1 8B, quantized) and validate the real 70B target elsewhere later, or (b) treat this machine as
   pure code-review/design work with no local model running at all? Lay out the real tradeoff, don't
   just pick one.

Guardrails, because I want you to actually push back if I'm about to do something the evidence
doesn't support:
- Verify VRAM/fit claims live (nvidia-smi, actual load attempts) before building on top of an
  assumption — including my own assumptions above.
- Whatever gets built, wire it through inference-core's existing `provider_order` /
  `FallbackChain` registration pattern (see `provider/fallback.rs`'s `"cohere"` arm as the freshest
  template) and the existing ZDR/residency gating — not as a bolted-on special case.
- Do NOT treat anything we do on this laptop as resolving, sizing, or substituting for Phase 7's
  bare-metal EU GPU procurement decision — that's a separate real-hardware decision I'm still
  waiting on, unrelated to what this laptop can prototype.
- If you hit a wall the doc didn't anticipate, tell me plainly rather than quietly working around it
  in a way that changes what's actually being validated.
```
