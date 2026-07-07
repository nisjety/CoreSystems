# Model Plane v2 Parity Migration

Current `apps/Model Plane` is the target runtime. `apps/Model Plane v2` is a donor for capability contracts and behaviours only; do not reintroduce the old Python monolith.

## Ownership Boundary

| Capability | Current owner | Notes |
|---|---|---|
| Provider credentials, model routing, embeddings generation, speech/image/translation/document intelligence adapters | Model Plane | Exposed through `inference-core` and `model-gateway`. |
| Vector storage, retrieval indexes, document storage, GraphRAG, wiki pages/source logs | Data Plane | Model Plane calls Data Plane for knowledge and retrieval. |
| Raw browser/web execution | Quarry | Model Plane plans browser actions and consumes observations. |
| Org/user/session auth issuance | Control Plane | Model Plane validates claims and policy context. |

## Parity Status

| v2 capability | Current Model Plane status | Implementation surface | Next step |
|---|---|---|---|
| Chat completion and streaming | Partial | `InferenceCore.Infer`, `InferenceCore.InferStream`, `/v1/ai/chat` | Add richer provider registry and batching. |
| Embeddings | Complete | `InferenceCore.CreateEmbedding`, `/v1/ai/embeddings`, Data Plane clients target `inference-core:9092` | Add production smoke coverage with real Azure OpenAI credentials. |
| Model listing | Partial | `InferenceCore.ListModels`, `/v1/ai/models`; now includes chat, embedding, TTS, STT, translation, image, vision, OCR, document intelligence, language analytics, realtime, and video provider entries | Move model/deployment inventory into capability-core persistence. |
| Image generation/analyze/OCR | Partial | `InferenceCore.GenerateImage`, `AnalyzeImage`, `ExtractImageText`; `/v1/ai/images`, `/v1/ai/images/analyze`, `/v1/ai/images/ocr`; OpenAI and Azure OpenAI image/vision adapters | Add Azure Document Intelligence read/OCR fallback and real provider smoke coverage. |
| Speech TTS/STT/stream/detect/list voices | Partial | `InferenceCore.SynthesizeSpeech`, `TranscribeSpeech`, `ListSpeechVoices`; `/v1/ai/speech`, `/v1/ai/speech/voices`; OpenAI, Azure OpenAI, Azure Speech TTS | Add streaming transcription, spoken-language detection, realtime voice session ownership, and `company_private` local STT routing for Velion Voice. |
| Translation/batch/detect/list languages | Partial | `InferenceCore.TranslateText`, `BatchTranslateText`, `DetectTextLanguage`, `ListTranslationLanguages`; `/v1/ai/translate`, `/v1/ai/translate/detect`, `/v1/ai/translate/languages`; Azure Translator plus Azure/OpenAI LLM fallback | Add transliteration only if product needs it, then run production Azure Translator smoke coverage. |
| Document intelligence layout/forms/receipts/invoices | Partial | `InferenceCore.AnalyzeDocument`; `/v1/ai/documents/analyze`, `/layout`, `/forms`, `/receipts`, `/invoices`; Azure Document Intelligence REST provider | Add real Azure Document Intelligence smoke coverage and richer typed invoice/receipt field normalization. |
| Language analytics sentiment/entities/key phrases/PII/summary | Partial | `InferenceCore.AnalyzeLanguage`; `/v1/ai/language`, `/sentiment`, `/entities`, `/key-phrases`, `/pii`, `/detect`, `/summary/text`; Azure AI Language plus Azure/OpenAI LLM fallback | Add production Azure AI Language smoke coverage and typed result normalization per operation. |
| Realtime sessions | Partial | `InferenceCore.CreateRealtimeSession`; `/v1/ai/realtime`, `/v1/ai/realtime/session`, `/v1/ai/realtime/models`; OpenAI realtime client-secret broker | Add real OpenAI realtime smoke coverage and browser/WebRTC client integration; add Azure realtime only after confirming product need. |
| Video jobs | Partial | `InferenceCore.CreateVideoGenerationJob`, `GetVideoGenerationJob`, `StreamVideoGenerationContent`; `/v1/ai/video/generate`, `/v1/ai/video/jobs/:job_id`, `/v1/ai/video/generations/:generation_id/content`, `/v1/ai/video/models`; Azure OpenAI Sora jobs provider | Add real Azure OpenAI Sora smoke coverage and durable artifact handoff if generated media must be retained. |
| Gemini/Mistral/Cohere/Ollama providers | Missing | Anthropic and OpenAI/Azure chat, OpenAI/Azure embeddings | Port provider adapters behind current Rust traits. |
| Agent snapshots/event history/control actions/subagents | Partial | Split Go/Rust services plus partial gateway RPCs | Map v2 agent-core APIs into orchestrator/session/execution services. |
| Hooks/MCP/plugins/commands | Partial | Capability-core schemas plus gateway in-memory registries | Move registries to capability-core persistence and implement execution. |

## Current Slice Completed

- Added embedding and model catalogue protobufs to `model_plane.v1.InferenceCore`.
- Regenerated Rust, Go, Python, and TypeScript protobuf bindings with `buf generate`.
- Implemented OpenAI/Azure embedding calls in Rust `inference-core`.
- Fixed inference-core provider hint propagation so provider-specific calls do not ignore the public contract.
- Added `/v1/ai/embeddings` and `/v1/ai/models` in Rust `model-gateway`.
- Retargeted Data Plane embedding clients from v2 `ai-core` to current `InferenceCore.CreateEmbedding`.
- Added speech RPCs to `InferenceCore` and retargeted gateway gRPC/HTTP speech requests to inference-core instead of gateway-local provider credentials.
- Added Azure Speech TTS, Azure OpenAI audio, OpenAI audio, speech model listing, and voice catalogue support.
- Planned Velion Voice retention modes: `cloud_zdr` for high-quality approved cloud STT/LLM processing, `company_private` for lower-quality self-hosted STT where raw audio/transcripts do not leave customer-controlled infrastructure, and explicit-save-only Teams transcript summarization.
- Added translation and language detection RPCs to `InferenceCore`, provider-backed Azure Translator/Azure OpenAI/OpenAI fallback logic, and gateway HTTP routes for translate, detect, and language catalogue operations.
- Added image generation, image analysis, and OCR RPCs to `InferenceCore`, provider-backed OpenAI/Azure OpenAI vision logic, and gateway HTTP routes for image generation, analyze, and OCR operations.
- Added document intelligence analysis RPCs to `InferenceCore`, an Azure Document Intelligence REST provider, and gateway routes for analyze/layout/forms/receipts/invoices while keeping durable document ingest in Data Plane.
- Added language analytics RPCs to `InferenceCore`, Azure AI Language and Azure/OpenAI LLM fallback providers, and gateway routes for sentiment/entities/key-phrases/PII/detect/summary.
- Added realtime session broker RPCs to `InferenceCore`, an OpenAI realtime client-secret provider, and gateway routes for session creation plus realtime model listing.
- Added video generation job RPCs to `InferenceCore`, an Azure OpenAI Sora jobs provider, and gateway routes for video job creation/status plus video model listing.
- Added a credential-safe streaming video content proxy so generated media can be fetched through Model Plane without exposing Azure OpenAI API keys to clients.
- Added Go capability-core parity status and seeded inference capabilities.
- Added Python `provider_research.parity` so parity status can be tested and reused by planning/eval tooling.

## Migration Rule

Every v2 feature must land in the current split service where it belongs:

| Runtime | What to port here |
|---|---|
| Rust | Provider-facing AI primitives, gateway routes, gRPC contracts, streaming, cost-sensitive execution paths. |
| Go | Capability registry, policy, runtime status, orchestration/control APIs, durable agent operational metadata. |
| Python | Research/eval/parity catalogues, graph/provider experimentation, non-production harnesses. |

Do not make Model Plane the source of truth for Data Plane objects. Embeddings generation belongs in Model Plane; embedding persistence and retrieval belong in Data Plane.
