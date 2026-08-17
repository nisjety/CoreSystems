# Provider & Privacy Strategy — what to copy, use, and skip

**Status:** **Decided** (§0.0) · **Date:** 2026-08-17
**Scope:** `apps/Model Plane` inference/provider layer + ZDR path; supplier strategy (Telenor AI Factory, Bineric, Azure, Google, xAI); privacy-tech adoption (Kiji, Venice)

---

## 0.0 Decision (settled 2026-08-17)

**Two product tiers. No model is processed outside the EU/EEA.**

| | **Tier A — Sovereign** | **Tier B — Frontier** |
|---|---|---|
| Provider | **Bineric now, Telenor later** | **Azure** + **Google Vertex (`eu`)** |
| Models | **Bineric's Norwegian/Nordic models ONLY** (Lynx, NorskGPT) — brokered third-party models hard-disabled | Azure OpenAI (**with ZDR**), Azure Foundry Claude, Gemini (EU endpoint) |
| Residency | Norway | EU/EEA |
| ZDR | TBD — gated on Bineric's DPA | ✅ Azure OpenAI · ❌ Claude (see §5 Phase 0.1) |
| Sold as | "Sensitive data stays in Norway" | "Best available models, EU-resident, no retention" |

**Grok is dropped.** It has no EU in-region inference path from any route (xAI direct, Azure Foundry, or Vertex). Dropping it is what makes the unqualified claim above possible. Reversible later as an explicitly-labelled exception — it's a config addition to the existing Azure Foundry footprint, not a new vendor — but it would reintroduce an asterisk on every residency statement.

**Claude stays in Tier B as a non-ZDR option.** The system already fails safe: `req.zdr && !provider.supports_zdr` skips the provider, so a ZDR request cannot reach Claude today. Tier B's honest claim is therefore *"ZDR available, served by Azure OpenAI"* until Phase 0.1 lands.

**Bineric → Telenor is a migration path, not a fork.** Both are OpenAI-compatible endpoints behind the same `ProviderRouter`; after the Phase 1 routing rewrite, moving Tier A from Bineric's hosted Lynx to our own Lynx on Telenor is a provider-registration change. Do not pay the GPU-operations cost before a customer is paying for it.

---

## 0. The five findings that change decisions

1. **Telenor AI Factory would *lose* you OpenAI and Anthropic, not extend you to Google and Grok.** It is GPU infrastructure (NVIDIA H100 DGX on Red Hat OpenShift AI, two Norwegian sites), and its advertised "190+ models" is a *deploy-from catalog* of **open-weight models only**. Gemini, Grok, GPT and Claude have no downloadable weights — no amount of sovereign GPU produces them. Telenor is an **addition** for a sensitive-data tier, never a replacement for frontier providers.
2. **Grok cannot be had with true EU in-region inference from any route.** xAI's own API has a DPA but no EU residency (us-east-1/us-west-2). Azure sells Grok as a Foundry model — but **Global Standard only in Europe; the Data Zone tier for Grok exists only for US regions.** Vertex offers `xai/grok-4.6` but xAI appears nowhere in Google's residency-commitment tables. Gemini *can* be had with real EU residency. Grok can't.
3. **No Claude path can currently serve a ZDR request at all.** `AnthropicProvider::capabilities()` hardcodes `supports_zdr: false` (`anthropic.rs:527`) with no builder to override it. Every ZDR request silently excludes Claude, direct *and* via Azure Foundry. This is a live gap in our own system and it is more consequential than anything on the supplier list.
4. **We already have a fail-closed PII masking seam — it just masks almost nothing.** `model-gateway/src/moderation.rs` treats capability-core as policy authority, treats client `features` as additive-only, and fails closed to redaction on every error path. That architecture is right. But `redact_pii` only handles emails and 13–19-digit runs, and it is applied at two call sites to `req.content` only — the **current user turn**. Conversation history, retrieval/grounding context, tool results, attachments and system prompts all reach the provider unredacted (deliberately, per the comment at `sse.rs:748`). Kiji parity is a *coverage and detector* problem, not an architecture problem.
5. **A second OpenAI-compatible provider cannot coexist today.** Provider identity is a hardcoded string `match` (`fallback.rs:315-410`), routing is a hardcoded two-family split (`provider_serves_model`, `fallback.rs:288-294`: anthropic-shaped serves `claude*`, OpenAI-shaped serves everything else), and hints use a fixed alias table (`fallback.rs:618-631`). Adding Telenor or Bineric needs **zero new adapter code** (`OpenAiProvider::new(key, Some(base))` already exists, `OPENAI_API_BASE` is already forwarded) — but both would register as `openai`, match the same hints, and first-registered would win. **This routing rewrite is the actual prerequisite for the whole supplier strategy.**

---

## 1. Current state of our provider layer (verified)

`ProviderRouter` (`inference-core/src/provider/mod.rs:324-354`) is a genuine trait — `infer`, `infer_stream`, `create_embedding`, `list_models`, `capabilities`. Exactly **two** implementors: `OpenAiProvider`, `AnthropicProvider`, each with an internal flavor enum. So the "four adapters" are 2 traits × 2 flavors: `openai`, `azure-openai`, `anthropic`, `azure-anthropic`. Nothing else exists — no Gemini, xAI, Mistral, Bedrock, Ollama or vLLM.

Beyond chat there are **seven more provider families with their own bespoke traits and `from_env()` chains** (Speech, Translation, Vision, DocIntel, LanguageAnalytics, Realtime, Video), sharing no capability struct, no ZDR gate, no residency gate with `ProviderRouter`. The provider layer is really eight unrelated layers.

**ZDR** is a single boolean, and the enforcement is genuinely good: it originates as a signed JWT claim, becomes `issuer_zdr`, and combines monotonically — `effective_zdr = issuer_zdr || request_zdr` (`auth.rs:193`), so a request can only tighten, never downgrade. Three fail-closed enforcement points: `reject_unattested_zdr_modality` fails 13 modality RPCs *before provider I/O* (`grpc.rs:158-168`); non-ZDR providers are skipped in all three chain paths with `ZdrUnavailable` when none remain; execution-core requires the delegated bearer's posture to *equal* the caller's. **But there is no privacy tier and no residency field on chat at all** — `EmbeddingResidency` with its `EU_AZURE_REGIONS` allow-list exists only on `EmbedRequest`, never on `InferRequest`.

**capability-core's `models` registry is decorative on the invoke path.** Row is `id, org_id, scope, provider, name, version, config_json, enabled, risk_level, lazy_load, description` — no privacy, residency or attestation column — and it is consulted only for capability listing/authz, never on invoke. The gateway's `/v1/models` explicitly proxies inference-core instead. Proof it's unused: the seed inserts `google/gemini-1.5-pro`, a model no adapter in the plane can serve, with `config_json = {}`.

---

## 2. Supplier strategy

The original two goals ("Telenor instead of Microsoft" + "add Google and Grok") are on **different axes** and cannot be served by one vendor. Sovereignty and model capability trade against each other; the settled answer (§0.0) is to offer both as explicit tiers and to drop the one model (Grok) that would have broken the EU boundary.

Underneath the two *product* tiers, keep four *enforcement* labels on the model row — `sovereign`, `eu_resident`, `zdr_contractual`, `global` — because Claude and Azure OpenAI differ inside Tier B and must not be conflated. After the Grok decision, **`global` has no members**; a model landing in that label is a bug, and the startup gate should refuse to register it.

**Bineric and Telenor are one stack, not two options.** Bineric's founder gives a customer testimonial on Telenor's own site; Bineric's logo is in Telenor's partner wall. Telenor is the sovereign GPU layer; Bineric is a model+product layer on it. Our choice is integration depth:

- **Via Bineric's API** — fastest. An OpenAI-compatible base-URL swap (pending confirmation of their wire format). We get a Nordic-language model plus their brokered catalog, and someone else operates the GPUs.
- **Direct on Telenor** — we run vLLM/KServe on their OpenShift ourselves. Full control, and we become our own inference provider: capacity planning, autoscaling, upgrades. Against a vendor with **Mon–Fri 09:00–17:00 CET, "best-effort during the initial phase"** support, no published SLA, no status page, no ISO 27001/SOC 2 claim, and no published DPA. Entry floors are genuinely startup-sized (1 GPU-hour on-demand, 1 GPU / 3 months reserved).

**Decided: start with Bineric's API, not direct Telenor.** It gets a Norwegian sovereign tier live in weeks instead of a quarter, and defers becoming a GPU operator until a deal pays for it.

### Two things that change hands when you go via Bineric

**The guarantee moves from Telenor to Bineric.** Telenor never sees a prompt either way — they rent GPUs; Bineric operates the model server, so **Bineric is the party that could log**. Telenor's "built to handle sensitive data" is a claim about Telenor's infrastructure and does not transfer to Bineric's API product. Every retention question is a Bineric question, and the GDPR chain becomes Verevon → Bineric (processor) → Telenor (sub-processor), which means Bineric must be named as a sub-processor in every Verevon customer DPA.

**"Bineric" is not a synonym for sovereign.** Their own positioning is *"Bineric Lynx + 30 leading models"* — those 30 brokered models are GPT/Claude/Gemini and by definition are **not** in Norway. Tier A must be pinned to Bineric's own models by an **enforced allow-list in provider config**, not a trust assumption or a model-hint default. A fallback or hint that silently routed a sovereign-tier request to Bineric's brokered GPT would leave the country and still look successful.

Also note the plain company-risk asymmetry a procurement officer will see: Telenor is a listed telco with the Norwegian state as its largest shareholder; Bineric is a startup. That argues for the Bineric→Telenor migration path being real, not theoretical.

**Telenor's real asset is procurement, not technology.** Telenor Norway runs its own security-act-regulated workloads there in a facility requiring security clearance, and DFØ's *Markedsplassen for skytjenester* ran an AI pilot through August 2026 with eleven public bodies — Datatilsynet, Digdir, Forsvarsdepartementet, Helsedirektoratet, Norges Bank, SSB, Tolletaten, Riksadvokaten, UiO. *"Our sensitive-data tier runs on Telenor AI Factory"* is a checkable sentence worth more than a certification badge in a Norwegian tender.

Two honesty notes: the named customer base is still small (Hive Autonomy, Capgemini, Telenor Norway, Sikri, Bineric, ayfie); and "fully Norwegian owned and operated" sits in tension with Accenture jointly operating the platform and Red Hat supplying the whole control layer. **Sovereignty here is jurisdictional and contractual, not air-gapped** — describe it that way.

### Gemini and Grok, concretely

**Gemini — real EU residency, available now.** Vertex AI's **EU multi-region (`eu`) jurisdictional endpoint** keeps ML processing inside EU member states (UK and Switzerland explicitly excluded), with the Cloud Data Processing Addendum as the Art. 28 DPA and a contractual Training Restriction. Committed for Gemini 3.7/3.6/3.5 Flash, 3.5/3.1 Flash-Lite, 2.5 Pro, 2.5 Flash/Flash-Lite and embeddings. **No Gemini 3.x Pro-class model appears in the residency table** — EU residency buys Flash-class 3.x or Pro-class 2.5, not the 3.x flagship. ZDR is a checklist, not a switch: request the abuse-monitoring exception, keep request-response logging off, set `store=false`, and **avoid Grounding with Google Search (3-day logs, non-disableable) and Maps grounding (30-day)** — use Web Grounding for Enterprise instead.

**Grok — cheapest to add, weakest guarantee.** Because Grok is a Foundry model sold by Azure, we can deploy `grok-4.3` / `grok-4-20-*` / `grok-4-1-fast-*` from our **existing Azure footprint under Microsoft's DPA**, with no new vendor relationship. But Grok in Europe is **Global Standard only** — the Data Zone tier for Grok exists only for US regions. Data at rest sits in the European geography; inference may process in any Azure region worldwide. So: ship it as an explicitly-labelled T3 model, excluded from any ZDR or residency-committed request, never a fallback target.

---

## 3. Kiji — copy it, but fix four defects and build a Norwegian layer

> **⚠ Masking is not our GDPR mechanism — it is defence in depth on top of one.**
> Under GDPR, pseudonymised data remains personal data (Recital 26) wherever
> re-identification is possible — and our mapping vault makes it re-identifiable
> *by design*. A masked prompt sent to Azure OpenAI still requires an Art. 28
> processor agreement and a transfer mechanism; those come from **Microsoft's DPA
> plus EU data-zone residency**, not from the proxy. Masking reduces breach impact
> and supports data minimisation under Art. 5(1)(c) — that is its real value.
> It also cannot protect *substance*: a contract's terms, a complaint's content or
> a condition described in prose are not named entities any NER model will catch.
> **Market it as "GDPR via Microsoft's DPA and EU residency, with PII masking as an
> additional safeguard" — never as "GDPR via our proxy."**


**License: Apache-2.0**, and the model *and* training dataset are Apache-2.0 on HuggingFace. Code can be **vendored with attribution**, not merely imitated. It is ~20.7k LoC of **Go** (not Python), running a **DeBERTa-v3-base token-classifier with a CRF head** through ONNX Runtime on local CPU.

**Three of its public claims don't survive reading the code:** the README/HF card/press all say *DistilBERT* while the training config sets `microsoft/deberta-v3-base` (184M params); the blog says *quantized* while `skip_quantization = true` and the app ships the **FP32 737 MB** ONNX; and the **94% F1** figure appears only in the blog with no dataset named, no per-entity breakdown, and no committed report. Its own e2e README calls the report *"a regression baseline, not a quality gate"* and **expects low recall on SSN/IBAN/credit-card/passport/national-ID** because synthetic formats don't match training conventions. Also: "16+ PII types" is really 26, and **all 26 are ML-detected — the regex detector ships with zero built-in patterns** and no checksum validation anywhere.

### Worth porting essentially verbatim: the streaming restorer
The most valuable ~900 lines in the repo (`proxy/streaming.go`, `codec_openai.go`, `codec_anthropic.go`): per-channel raw carry buffers; hold back **only** the suffix that is a proper prefix of some dummy or an incomplete UTF-8 rune; emit restored text immediately and never rescan; flush on `content_block_stop` / `*.done` / `*.completed`; synthetic tail-flush on EOF; and critically a **JSON-escaped restore variant** for `input_json_delta` / `function_call_arguments` so a restored quote can't break the tool JSON the client reassembles. Plus `BuildRestorer`'s single-pass longest-key-first replacer, with its documented reason: a generated dummy can coincide with another mapping's original, so chained `ReplaceAll` corrupts output. In Rust use `aho-corasick` with `MatchKind::LeftmostLongest` — **never a loop of `str::replace`**.

### Four defects to fix rather than inherit
1. **No mutex on the ONNX session.** `ONNXModelDetectorSimple` shares `session`, `inputTensor`, `outputTensor` as mutable state across a 10-RPS/burst-20 server — concurrent detections can interleave and produce wrong spans. Use a bounded session pool behind a semaphore.
2. **Globally-unique plaintext vault.** `original_pii TEXT UNIQUE` in unencrypted SQLite, plus a `logs` table storing `request_original` and `response_original` **on by default**. In multi-tenant that global uniqueness is a **cross-tenant leak**: a dummy minted for org A will restore inside org B's response. Our key must be `(org_id, thread_id, original)`, mappings ephemeral in Redis with a TTL bound to the conversation or encrypted per-org, and under `req.zdr` **memory-only for the request lifetime**.
3. **Request-side coverage far narrower than its own docs claim.** Anthropic masking walks `messages[]` and masks only `type=="text"` — `tool_use`, `tool_result` and images are skipped, and the top-level `system` field and `tools` definitions are never masked. Their own flagship demo (`claude "summarize the customer complaints in support_log.txt"`) sends the file contents to the model **unmasked** as a `tool_result`. This is exactly our gap too (§0.4).
4. **Exact-substring restoration fails silently.** If the model translates, reformats, re-hyphenates or paraphrases a dummy, restoration breaks and the fake value ships to the user with nothing detecting it.

### Build the Norwegian identifier layer FIRST
**Norwegian is not supported by Kiji's model and can't be bolted on** — training languages are en/de/fr/es/nl/**da**, on an English-pretrained DeBERTa-v3-base (not mDeBERTa/XLM-R), so any Norwegian performance is transfer-by-accident through Danish. Their own issue #457 proposes moving to XLM-RoBERTa.

The cheapest work has the highest precision. Checksum-validated detectors, days of work, near-zero false positives, strictly better than any NER for these classes:
- **fødselsnummer** — 11 digits, two mod-11 check digits, plus D-number (day+40) and H-number (month+40) variants
- **organisasjonsnummer** — 9 digits, mod-11 weights 3,2,7,6,5,4,3,2
- **kontonummer** — 11 digits mod-11 · **Norwegian IBAN** — `NO` + 2 check + 11, ISO 7064 mod-97 · **KID** — mod-10/mod-11
- `+47` 8-digit phone with valid prefix ranges · **postnummer** cross-checked against the Posten list · kommunenummer · gnr/bnr · HPR-nummer · MVA-nummer
- Plus Luhn for cards and ISO 7064 for all IBANs — both trivial, both missing from Kiji

Only then the ML layer: keep Kiji's **26-label BIO taxonomy** and its `label_mappings.json` + `crf_transitions.json` sidecar contract (clean, portable JSON), but swap the encoder for one that has seen Norwegian — mDeBERTa-v3-base or XLM-RoBERTa-base, or NorBERT-3/NB-BERT-base if Norwegian-only is acceptable. Bootstrap data with Kiji's own Metaflow/Label-Studio pipeline; the blog puts six-language synthetic generation at ~$50, so a Norwegian slice is a 1–2 week task, not research.

### Rust is the better host, not the harder one
Every dependency Kiji reaches through cgo is Rust-native upstream: `tokenizers` 0.23.1 *is* the library Go wraps via FFI; `ort` 2.0.0-rc.13 binds ONNX Runtime 1.28; and **`candle-transformers` 0.11.0 already ships `models/debertav2.rs` with `DebertaV2NERModel` and `NERItem`** plus a working example — Kiji's exact architecture in pure Rust, no libonnxruntime, no `.dylib` path probing. The CRF Viterbi decode is ~55 lines to port. The regex detector ports 1:1 (both Go and Rust are RE2).

### Where it must sit
**In `inference-core`, not the gateway.** Add an `mp-privacy` crate exposing `detect(text) -> Vec<Span>`, `mask(req, ctx) -> (MaskedReq, MappingHandle)`, `restore_stream(handle)`, called from `provider/openai.rs` and `provider/anthropic.rs` immediately before the `reqwest` send and immediately after the response stream is obtained. That is the only point where UI chat, agent execution loops, ingestion summarisation, evals and support workers all converge — and it already has `zdr` threaded through. Implementing it in the Verevon gateway would violate our own *"gateway proxies, never reimplements"* rule and silently miss every non-UI caller.

**Add the two guards Kiji lacks**, which turn "we mask" from a claim into a measurable invariant: after restoration, assert no dummy from the mapping remains in delivered text (metric + audit event on a hit, not silent shipping); and on egress, scan the **masked** outbound body for any original from the mapping and fail closed if found.

---

## 4. Venice — copy the privacy *tiering*, not the TEE

**Decision: do not build toward TEE.** In August 2026, "attested" and "frontier" are mutually exclusive:

- Azure has exactly **one** confidential GPU SKU (`Standard_NCC40ads_H100_v5`, single H100 NVL, no multi-GPU, no multi-node) — and per SKU trackers it is in **West Europe only, not Sweden Central, not North Europe, not Norway**. No confidential H200/B200 on Azure at all.
- Google is ahead (Confidential Space on H100 GA April 2026; Confidential VMs/GKE on G4 Blackwell in preview across all G4 regions) but still preview on workstation-class GPUs. AWS is out entirely — Nitro Enclaves have no PCI passthrough, therefore no GPU.
- **No frontier lab ships weights into a customer-controlled enclave**, and Anthropic's own confidential-inference paper explains why (mutual attestation; weights decrypted only inside a loader that never releases them) — and notably even in that design Anthropic holds plaintext at the API boundary. The one genuine first-party attested service is **Azure AI Confidential Inferencing for Whisper** — speech-to-text only, preview since Oct 2024.
- Attestation verification is a **subsystem, not a feature**: nonce/session-key-bound quote, full PCK chain to a pinned Intel root, DCAP collateral from Intel PCS or a PCCS (never from the attested party) with caching/expiry/revocation, an explicit policy on `OUT_OF_DATE`/`SW_HARDENING_NEEDED`, RTMR/MRTD against an allow-list built from reproducible builds, plus a separate NVIDIA leg. The independent Venice audit found exactly the mistakes this invites: an unpinned mutable image tag inside a measured manifest, and a **self-reported `nvidia.valid` boolean from the very party the encryption defends against**.

**Write down the two triggers that reopen this**, assign an owner, review quarterly: (a) confidential Blackwell multi-GPU GA in an Azure or GCP **EU** region; (b) any frontier lab shipping customer-verifiable attested inference for its own hosted models. Neither has an announced date.

### What to copy instead

**Venice's genuinely portable idea is the four-mode privacy tier exposed programmatically** — their `/models` endpoint carries `model_spec.privacy` and `supportsTeeAttestation`/`supportsE2EE` so a caller can assert the tier *before* sending. That maps directly onto our gap (§1: no tier, no residency field on chat).

Replace our `zdr: bool` with a tri-state-or-better **privacy tier on the model row and on `InferRequest`**, and put it in the one place that is currently decorative — capability-core's `models` registry — making it the invoke-path source of truth it pretends to be:

| Tier | Meaning | Backed by |
|---|---|---|
| `sovereign` | Processed and stored in Norway | T1: Telenor/Bineric |
| `eu_resident` | ML processing committed to EU/EEA + Art. 28 DPA | T2: Azure data-zone, Vertex `eu` |
| `zdr_contractual` | No retention, contractually, evidence-bound | T2 with abuse-monitoring exception |
| `global` | No residency commitment | T3: Grok, Global Standard deployments |

And copy Venice's **honesty about mode-vs-capability**: their E2EE mode disables tool calling, web search and memory — because a provider that cannot read the prompt cannot orchestrate against it. Our equivalent: **a `sovereign`-tier request may reach a smaller model catalog**, and the UI must say so rather than silently downgrading quality.

### The invariant this protects
Venice's tool-calling limitation is an *implementation* consequence of provider-side orchestration, not a law. Client-side orchestration is fully compatible with an encrypted model hop: the model emits an encrypted `tool_call`, we decrypt in our own trust domain, execute, re-encrypt the result.

**Our "model proposes, Quarry-v2 executes or rejects" rule is already exactly the right shape.** Codify it as a written cross-plane invariant and extend it explicitly to search and memory: **never adopt provider-side orchestration (OpenAI Responses-API built-in web search, provider-side memory) as the only path for a privacy-tier customer.** That single design choice is what would make a future encrypted model hop impossible.

---

## 5. Sequencing

**Phase 0 — fix what's broken in our own claims (do first; independent of every supplier).**
1. **Give `AnthropicProvider` a ZDR builder.** Today no Claude path can serve a ZDR request (`anthropic.rs:527`). Either wire the Azure-Foundry Claude route's real posture, or make the exclusion explicit and visible instead of silent.
2. **Upgrade `AZURE_OPENAI_ZDR_CONFIRMED` from a boolean to bound evidence** — Azure resource id + Modified Abuse Monitoring approval reference + effective date + reviewer, hashed to a non-placeholder digest; refuse to register `supports_zdr` unless it validates. Right now `config.rs:96-98` is the same self-reported-boolean antipattern the Venice audit called out, pointed at ourselves.
3. **Fail-loud startup gate rejecting Global/Worldwide Azure deployment types**, mirroring the existing deny-by-default EU embedding gate at `config.rs:100-104`. A `Global` deployment silently voids the entire EU-boundary claim and is the most likely way we break our own promise.
4. **Apply for Azure Modified Abuse Monitoring now** — it needs an EA/MCA plus a Microsoft account team. Get the residual-operational-retention question answered *in writing* for our specific resource; Microsoft's own support answers currently contradict each other.

**Phase 1 — provider routing rewrite (the prerequisite).** Replace the hardcoded name `match`, the two-family `provider_serves_model` split, and the fixed hint alias table with a registry-driven provider identity + capability declaration. Make capability-core's `models` registry the invoke-path source of truth and add the privacy-tier column. Without this, no new supplier can be addressed distinctly.

**Phase 2 — add the two decided providers.**
- **Bineric as Tier A** — an OpenAI-compatible provider registration (zero adapter code if their wire format confirms), with a **hard model allow-list** restricted to their Norwegian/Nordic models and brokered models refused at registration, not merely unselected. Blocked on the Bineric call (§6).
- **Google Vertex `eu` for Gemini** — a genuinely new wire protocol, so a real adapter (~7 files). Constrain the catalog to what Google actually commits to EU ML-processing: Gemini Flash-class 3.x or Pro-class 2.5. **No Gemini 3.x Pro-class model appears in Google's residency table**, so it must not be selectable in Tier B.
- **Grok: not implemented.** Leave a single ADR line recording that it was evaluated and dropped for lack of any EU in-region path, so it isn't re-proposed.

**Phase 3 — masking layer.** `mp-privacy` crate in inference-core: Norwegian checksum detectors → coverage fix (system prompt, history, retrieval context, tool results, attachments) → ported streaming restorer → the two leak guards. Multi-tenant mapping keys and ZDR memory-only from line one.

**Phase 4 — the provenance receipt, as a product feature.** Per-request: provider, deployment name, region/data zone, retention posture, model, privacy tier — visible in the UI and exportable. orchestrator-core already stamps `zdr` on run envelopes (`activities.go:367-401`); surface it. **This is the defensible differentiator, and it is strictly more useful to a Norwegian buyer than an attestation quote they cannot verify.**

**If a named deal demands technical non-access before any of this is ready:** resell rather than build. Pilot **Privatemode AI** (Edgeless Systems) behind the existing `ProviderRouter` trait as a `confidential` provider — EU-hosted, OpenAI-syntax, open source with reproducible builds, transparency logs and end-to-end remote attestation, buyable off a price list today. Days of integration versus a quarter of attestation engineering.

---

## 6. Blockers — the Bineric call

Tier A cannot ship until these are answered. Question 3 is the one that decides whether Tier A is a *control* or a *hope*: if routing can't be hard-pinned, sovereignty isn't enforceable and direct Telenor becomes the only honest way to make the claim.

1. **Is your API OpenAI wire-compatible?** (Base-URL swap vs. a new adapter.)
2. **Do you offer an Art. 28 DPA, and does it include a no-logging / zero-retention term for API traffic?** (Gates whether Tier A can carry the ZDR claim at all.)
3. **Which models run on Telenor AI Factory, and can our API key be pinned to those only — with brokered third-party models hard-disabled server-side?**
4. **Do you log prompts or completions at all** — abuse monitoring, debugging, model improvement? Retention period?
5. **May we name Telenor AI Factory in customer-facing material** as the underlying infrastructure?

## 7. Other open questions

1. **Telenor's DPA, SLA and certification roadmap** — nothing published. Required before the later direct-Telenor migration, not before Bineric.
2. **Whether Gemini 3.x Pro-class enters Google's EU residency table** — currently absent; determines whether Tier B can ever offer a flagship Gemini or stays on Flash-class 3.x / Pro-class 2.5.
3. **Azure Modified Abuse Monitoring** — application started? Needs an EA/MCA and a Microsoft account team. Get residual-operational-retention answered in writing for our specific resource; Microsoft's own support answers contradict each other.
4. **Norwegian PII training data licensing** — synthetic generation avoids the problem, but confirm no real fødselsnummer ever enters a training set.
