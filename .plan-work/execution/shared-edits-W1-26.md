## WS02-S10.code
shared-edit apps/Model Plane/rust/services/inference-core/src/provider/mod.rs: after line 171 (`pub prompt_tiers: mp_contracts::prompt_tier::PromptTierPlan,`) and before the image_parts doc comment (line 172), add the doc comment `/// Provider prompt-cache routing key from the proto (mp-contracts prompt_tier::cache_scope_key); empty names none. Read only by the OpenAI adapter, behind INFERENCE_OPENAI_PROMPT_CACHE_KEY, on the first-party endpoint, and never on a ZDR request.` and the field `pub cache_scope_key: String,`

## WS02-S10.code
shared-edit apps/Model Plane/rust/services/inference-core/src/grpc.rs: in the Ok(provider::InferRequest { ... }) literal that starts at line 1228, after the prompt_tiers expression (its last line `.unwrap_or_default(),` is line 1257) and before `org_id: principal.org_id.clone(),` (line 1258), add `cache_scope_key: req.cache_scope_key.clone(),`

