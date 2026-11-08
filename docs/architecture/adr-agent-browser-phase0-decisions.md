# ADR — Agent-first browser: Phase 0 decision proposals

Status: **proposed** (owner/plane sign-off pending; nothing here is decided).

These proposals gate the agent-first browser plan (`PLAN.md`). Until signed off, phases that depend on a decision stay behind flags and do not change cross-plane semantics.

## 0.1 Slice 3 — permission catalogue (Control Plane owner)

- Proposal: per-site levels `none/read/act` stay; add `download`/`upload`/`record` as separate org-restrictible defaults with per-site exceptions, surfaced in the settings hub already shipped. Any new level requires broker enforcement + Quarry gate, never SPA-only.
- Sign-off needed: Control catalogue + org-core migration owner. Build below the line is blocked until then.

## 0.2 Slice 9 — secret custody (ADR-2, Model/Ingestion owner)

- Proposal: secrets live only in custody; the model sees typed placeholders (`<secret>key</secret>`); Quarry carries values via `fill_secret`; receipts carry booleans, never values. SPA redaction already shipped as defence in depth.
- Sign-off needed: ADR-2. `fill_secret` below the line is blocked until then.

## 0.3 Slice 8 — single editor (R-27) + WS3.K tuning (Model Plane owner)

- Proposal: planner view = numbered element table with closed op set; wait constants (combobox 200 ms / 2 frames) adopted as tuning data, not a new gate. Interactive-only SPA filter already shipped.
- Sign-off needed: R-27 single-editor decision. Planner depth below the line is blocked until then.

## 0.4 Slice 2 — navigation audit subject (session-core owner)

- Proposal: session-core owns `verevon.audit.v2.model.*` publication for navigation runs; the frontend gateway never publishes. Durable `run_id`/`thread_id` are already returned.
- Sign-off needed: Model Plane owner. Below the line is an audit-publisher integration, not a UI change.

## 0.5 Slice 4 — annotation attachment (WS9/WS11, Model Plane owner)

- Proposal: annotation = user-authored comment on viewport coordinates, attached to the planner as a governed attachment; page region ephemeral; ZDR: comments user-authored only. SPA clipboard draft already shipped.
- Sign-off needed: attachment contract before planner wiring.

## 0.6 Slice 5 — server history retention (Data/Ingestion owner)

- Proposal: WS8.B metadata receipts are the store; ZDR orgs keep no entries; purge hooks into existing `clearHomeLocalContent`. Local bookmarks already shipped.
- Sign-off needed: retention design before server log.

## 0.7 Slice 10 — replay semantics (WS10/WS8, Model Plane owner)

- Proposal: zero model calls per replayed step; per-step approval preserved; fail closed at first mismatch. `buildReplay` already shipped.
- Sign-off needed: replay semantics before step-picker UI.

## 0.8 Agent session lifetime (Control + Model owners)

- Proposal: one long-lived Quarry session per (org, user) ("Verevon's browser"); per-task grants stay 5-min windows on the session; per-task isolation = new run per task, no shared page state in the model's context. SPA-side reuse (this worktree) only ever reuses the *same-org, same-user, same-origin* retained session and closes otherwise — so no new cross-plane semantics are introduced before sign-off.

---

*Phase 1/2 of the plan proceed on the conservative SPA side of 0.8; all cross-plane phases wait for their sign-offs above.*
