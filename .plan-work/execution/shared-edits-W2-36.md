## WS14-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs (CRLF; reviewer item 1; line numbers are current, but they shift as you insert, so apply bottom-up or by anchor text): (a) after line 137 `    ("execute_granted", refused_feature("agentic")),` add `    ("approved_plan_id", refused_feature("agentic")),`; (b) after the block at lines 324-326 (`    if request.execute_granted {` / `        return feature("agentic");` / `    }`) add `    if request.approved_plan_id.as_deref().is_some_and(|id| !id.trim().is_empty()) {` / `        return feature("agentic");` / `    }`; (c) in invoke_field_names(): after line 244 `        execute_granted: _,` add `        approved_plan_id: _,`, and after line 286 `        "execute_granted",` add `        "approved_plan_id",`.

## WS14-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs (CRLF; reviewer item 2; test-only, inside mod tests at line 518): after line 600 `            execute_granted: false,` (inside the InvokeRequest literal that starts at line 568) add `            approved_plan_id: None,`.

## WS14-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifacts.rs (CRLF; reviewer item 3; no owner in the W2-36 lists, so assign it): line 27 `use crate::sse_events::ChatEvent;` becomes `use crate::sse_events::{AttachmentPreview, ChatEvent};`; in attachment_event (line 482) after line 488 `    check: Option<mp_contracts::office_verification::OfficeVerification>,` add `    preview: Option<AttachmentPreview>,`; in its literal after line 496 `        check,` add `        preview,`; in the test after line 809 `            None,` add a second `            None,` (seventh argument), and after line 817 `                check,` add `                preview: _,`.

## WS14-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse_events.rs (CRLF; WS19-S5-owned; test-only): line 3088 `    use super::{ChatEvent, RetentionClass, ServedBy};` becomes `    use super::{AttachmentPreview, ChatEvent, RetentionClass, ServedBy};`. tool_loop.rs:719 needs no edit once the artifacts.rs item lands.

## WS19-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/code_interpreter.rs (measure CRLF first): in struct CodeResult, after line 200 (`verification_unchecked: usize,`), add a doc comment, `#[serde(skip_serializing_if = "Vec::is_empty")]` and `previews: Vec<office_verify::FilePreview>,`. After line 730 (`verification_unchecked: office.unchecked,`) add `previews: office.previews,`. After line 1019 (`verification_unchecked: 0,`) add `previews: Vec::new(),`. This is the key the gateway reads as payload["previews"]. CodeResult and FilePreview both derive Serialize only, so this compiles.

## WS19-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifacts.rs (measure CRLF first): (a) after line 488 (`check: Option<mp_contracts::office_verification::OfficeVerification>,` in `pub fn attachment_event(`, line 482) add `    preview: Option<crate::sse_events::AttachmentPreview>,`; (b) after line 496 (`        check,` in the literal) add `        preview,`; (c) after line 809 (the test call's `            None,`) add a second `            None,`; (d) after line 817 (`                check,` in the test pattern) add `                preview: _,`. tool_loop.rs already calls it with seven arguments.

## WS19-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (LF; re-find by text if WS14-S2 edits move it): in the image literal `let attachment = crate::sse_events::ChatEvent::Attachment {` (line 7340), after `check: None,` (line 7346) add `preview: None,`.

## WS19-S5
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx (LF): add `import { officePdfRenderer } from './ArtifactOfficePreview'` beside line 39 (`import { extraArtifactRenderers } from './artifact-renderers'`). After the last import add `extraArtifactRenderers.register(officePdfRenderer)`. Replace line 424 (`{(component) => <Dynamic component={component()} artifact={displayed()} />}`, under `<Match when={extraRenderer()?.component}>` at line 423) with `{(component) => <Dynamic component={component()} artifact={displayed()} file={props.item.file} fallback={() => (<Show when={fileMeta()}>{(meta) => <ArtifactFileCard meta={meta()} title={title()} />}</Show>)} />}`.

## WS19-S5
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.ts (LF): (a) line 68: add `type GeneratedFilePreview,` beside `type GeneratedFile,`; (b) beside normalizeGeneratedFileCheck (line 1642) add `export function normalizeGeneratedFilePreview(value: unknown): GeneratedFilePreview | undefined`, returning undefined unless mime is application/pdf and url starts with `data:application/pdf;base64,`, pages is a number, fonts_substituted is a boolean and thumbs is an array; it maps fonts_substituted to fontsSubstituted and keeps only `data:image/png;base64,` thumbs; (c) in normalizeGeneratedFile's event type, after line 1664 (`check?: unknown`) add `preview?: unknown`; (d) after line 1667 add `const preview = normalizeGeneratedFilePreview(event.preview)`; (e) after line 1674 (`...(check ? { check } : {}),`) add `...(preview ? { preview } : {}),`; (f) add a guard test beside chat-normalizers.generated-file-check.test.ts.

## WS19-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts (LF): after line 421 (`check?: unknown` in ChatAttachmentEvent) add `preview?: unknown`; after line 1689 (`check: value.check,`) add `preview: value.preview,`; after line 1931 (`check: payload.check,`) add `preview: payload.preview,`. use-chat-controller.ts needs no change: lines 3632-3633 pass the whole event to normalizeGeneratedFile.

## WS19-S5
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css: add `.verevon-chat-artifact-office` (column flex), `.verevon-chat-artifact-office__notice` (small muted text) and `.verevon-chat-artifact-office__pages` (horizontal scroll row; buttons outlined when aria-current="page"; thumbnail images about 96px high), next to `.verevon-chat-artifact-frame` (line 28193). The frame reuses that class.

## WS21-S5b
shared-edit apps/Model Plane/deploy/docker-compose.yml (optional, documentation only): execution-core environment line EXECUTION_DO_BROWSER_ACT_ENABLED=${EXECUTION_DO_BROWSER_ACT_ENABLED:-false}. Do NOT set it to true anywhere until K3-6 passes.

