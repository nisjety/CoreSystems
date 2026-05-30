# Quarry Lab (Python)

**Lab-only**. Never in the hot path. Outputs feed back into Rust/Go via artifacts or static config, never via runtime call.

Scope:

- extraction prompt + schema experiments
- anti-bot/evasion strategy experiments
- ML-based driver-selection experiments
- benchmark + eval harnesses (see `docs/ROADMAP.md` Phase 8)

Structure (proposed):

```
lab/
├── extraction/        # prompt + schema sweeps, scored against gold set
├── antibot/           # challenge-fingerprint learning
├── driver_select/     # static vs browser classifier
└── evals/             # benchmark suites, scoreboards, leaderboards
```

No services here. Notebooks, scripts, reports only. Python env managed via `uv` or `poetry` per subfolder.
