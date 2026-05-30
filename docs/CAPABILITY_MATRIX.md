# Capability Matrix — Legacy vs MP V2

**Last Updated**: 2026-04-07

## Feature Status Legend

| Status | Meaning |
|--------|---------|
| ✅ Active | Fully implemented and operational |
| ⚡ Partial | Config/skeleton exists, runtime incomplete |
| 📋 Placeholder | Env vars only, no implementation |
| ❌ Missing | Not present |
| 🔄 Duplicated | Exists in both; MP V2 is canonical |

## Core Capabilities

| Capability | Legacy ai-core | MP V2 ai-core | MP V2 Agent Core | Data Plane | Target Owner |
|---|---|---|---|---|---|
| **Chat / Completions** | ✅ Active | ✅ Active | — | — | MP V2 ai-core |
| **Streaming Chat** | ✅ Active | ✅ Active | — | — | MP V2 ai-core |
| **Pipeline (Layers 1-10)** | ✅ Active (L1-L10) | ✅ Active (L01-L10) | — | — | MP V2 ai-core |
| **Layer 0 (Multimodal)** | ❌ Missing | ❌ Missing → ✅ Adding | — | — | MP V2 ai-core |
| **Model Routing** | ✅ Active | ✅ Active | — | — | MP V2 ai-core |
| **Content Safety** | ✅ Active | ✅ Active (Azure SDK) | — | — | MP V2 ai-core |
| **Intent Classification** | ✅ Active | ✅ Active | — | — | MP V2 ai-core |

## gRPC Services

| Service | Legacy ai-core | MP V2 ai-core | Target |
|---|---|---|---|
| **ChatService** | ✅ Active (many RPCs) | ✅ Active | MP V2 |
| **SpeechService** | ✅ Active | ✅ Active | MP V2 |
| **DocumentService** | ✅ Active | ✅ Active | MP V2 |
| **ImageService** | ✅ Active | ❌ Missing → ✅ Adding | MP V2 |
| **TranslationService** | ✅ Active | ❌ Missing → ✅ Adding | MP V2 |
| **Health Check** | ✅ Active | ❌ Missing → ✅ Adding | MP V2 |
| **Server Reflection** | ✅ Active | ❌ Missing → ✅ Adding | MP V2 |
| **Keepalive/Sizing** | ✅ Active | ❌ Missing → ✅ Adding | MP V2 |

## Document Processing

| Capability | Legacy ai-core | MP V2 ai-core | Target |
|---|---|---|---|
| **Azure Document Intelligence** | ✅ Active | ✅ Active | MP V2 (via Analyzer) |
| **Mistral Document AI** | ✅ Active (hybrid routing) | ⚡ Partial (endpoint only) | MP V2 (via Analyzer) |
| **Content Understanding** | 📋 Placeholder | 📋 Placeholder → ✅ Adding | MP V2 (via Analyzer) |
| **Analyzer Abstraction** | ❌ Missing | ❌ Missing → ✅ Adding | MP V2 |

## Speech / Audio / Video

| Capability | Legacy ai-core | MP V2 ai-core | Target |
|---|---|---|---|
| **TTS (Azure Speech)** | ✅ Active | ✅ Active | MP V2 |
| **TTS (OpenAI)** | ✅ Active | ✅ Active | MP V2 |
| **ASR / Transcription** | ✅ Active | ✅ Active | MP V2 |
| **Video Generation (Sora)** | ⚡ Partial | ⚡ Partial (submit/poll) | MP V2 |
| **Realtime Conversation** | ⚡ Partial | ⚡ Partial (session create) | MP V2 |

## Translation / Language

| Capability | Legacy ai-core | MP V2 ai-core | Target |
|---|---|---|---|
| **Azure Translator** | ✅ Active | ✅ Active | MP V2 |
| **Azure AI Language** | ⚡ Partial | ⚡ Partial (analytics endpoint) | MP V2 |

## Agentic RAG

| Capability | Legacy ai-core | MP V2 ai-core | MP V2 Agent Core | Data Plane | Target |
|---|---|---|---|---|---|
| **Planning Agent** | ❌ | ❌ | ❌ → ✅ Adding | ✅ Active | Agent Core |
| **Routing Agent** | ❌ | ❌ | ❌ | ✅ Active | Data Plane |
| **Retrieval** | ❌ | ❌ | ❌ | ✅ Active | Data Plane |
| **Reranking** | ❌ | ❌ | ❌ | ✅ Active | Data Plane |
| **Reflection Agent** | ❌ | ✅ (L08b) | ❌ → ✅ Adding | ✅ Active | Agent Core |
| **Synthesis Agent** | ❌ | ✅ (service) | ❌ → ✅ Adding | ✅ Active | Agent Core |
| **RAG Orchestrator** | ❌ | ❌ | ❌ → ✅ Adding | ✅ Active | Agent Core |

## Infrastructure

| Capability | Legacy ai-core | MP V2 ai-core | Target |
|---|---|---|---|
| **Readiness Endpoint** | ⚡ Basic health | ❌ → ✅ Adding | MP V2 |
| **Feature Activation Report** | ❌ Missing | ❌ → ✅ Adding | MP V2 |
| **Env Normalization** | ❌ (673-line .env) | ❌ → ✅ Adding | MP V2 |
| **Observability (request_id)** | ⚡ Partial | ✅ Active (pipeline) | MP V2 |
