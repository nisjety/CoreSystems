import { readdirSync, readFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { isModelEntitlementCode } from '@/shared/api/model-entitlement-refusals'
import { isZdrRefusalCode } from '@/shared/api/zdr-refusals'
import { BLOCK_REASON_COPY, ERROR_COPY, describeApiError, translateApiError } from './errors'

// Every error code the Verevon gateway can send to the SPA has end-user copy in
// `errors.ts`, or a reasoned entry below saying why it needs none. The scan reads
// every production Rust file under `apps/gateway/src` (domains, middleware, upstream
// proxy, onboarding). A test module that a `#[cfg(test)]` declaration names is not
// production code, so it is not read. A code added in ANY production file, in one of
// the shapes listed below, fails here until it is given copy or an explicit exemption.
// A code a helper builds from a variable has no shape to scan: those are pinned one by
// one in HELPER_EMITTED.
// List of the codes and where each one comes from:
// `wf-V3-spa-errors-codes.md` in the side-session notes.

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

const gatewaySrc = resolve(process.cwd(), 'apps/gateway/src')

function rustFiles(dir: string): string[] {
  const files: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = `${dir}/${entry.name}`
    if (entry.isDirectory()) files.push(...rustFiles(path))
    else if (entry.name.endsWith('.rs')) files.push(path)
  }
  return files.sort()
}

/** The file's production code: CRLF normalised, every `mod ...tests { }` block removed. */
function productionCode(path: string): string {
  const kept: string[] = []
  let skipping = false
  for (const line of readFileSync(path, 'utf8').replace(/\r\n/g, '\n').split('\n')) {
    if (skipping) {
      if (line === '}') skipping = false
      continue
    }
    // browser.rs writes its test module as `pub(crate) mod tests {`; the visibility is optional.
    if (/^(?:pub(?:\([a-z]+\))? )?mod [a-z_]*tests? \{$/.test(line)) {
      skipping = true
      continue
    }
    kept.push(line)
  }
  return kept.join('\n')
}

/**
 * The files a `#[cfg(test)] mod name;` declaration names, with or without a `#[path = ".."]`. They are
 * compiled only for the tests, so they are not production code, whatever they are called (`*_tests.rs`,
 * `tests.rs` or another name). Each path is resolved as rustc resolves it: a `#[path]` from the folder of
 * the declaring file; otherwise `name.rs` or `name/mod.rs` in the folder named after the declaring file
 * (the folder itself for a `mod.rs` or a crate root).
 */
function testOnlyFiles(files: string[]): Set<string> {
  const known = new Set(files.map((file) => resolve(file)))
  const testOnly = new Set<string>()
  for (const file of files) {
    const text = readFileSync(file, 'utf8').replace(/\r\n/g, '\n')
    const declaringDir = dirname(resolve(file))
    const name = basename(file)
    const childDir = name === 'mod.rs' || name === 'main.rs' ? declaringDir : join(declaringDir, basename(file, '.rs'))
    for (const match of text.matchAll(/^#\[cfg\(test\)\]\n((?:#\[[^\n]*\]\n)*)(?:pub(?:\([a-z]+\))? )?mod ([a-z_0-9]+);$/gm)) {
      const explicit = /^#\[path = "([^"]+)"\]$/m.exec(match[1] ?? '')
      const moduleName = match[2] ?? ''
      const candidates = explicit
        ? [join(declaringDir, explicit[1] ?? '')]
        : [join(childDir, `${moduleName}.rs`), join(childDir, moduleName, 'mod.rs')]
      const found = candidates.map((candidate) => resolve(candidate)).find((candidate) => known.has(candidate))
      if (found) testOnly.add(found)
    }
  }
  return testOnly
}

const gatewayFiles = rustFiles(gatewaySrc)
const testOnly = testOnlyFiles(gatewayFiles)
const sources = new Map(
  gatewayFiles
    .filter((path) => !testOnly.has(resolve(path)))
    .map((path) => [path.slice(gatewaySrc.length + 1), productionCode(path)]),
)

/**
 * Every `const|static NAME: TYPE = ..;` of a source text whose type is a table of strings: a slice `&[&str]`, a
 * fixed-size array `[&str; N]`, or a table of tuples led by a string, `&[(&str, u16, &str)]` or `[(&str, &str); N]`.
 * `tuples` says which one it is; `body` is the text between the brackets, multi-line tables included.
 */
function stringTables(source: string): { name: string; body: string; tuples: boolean }[] {
  const tables: { name: string; body: string; tuples: boolean }[] = []
  const declaration =
    /\b(?:const|static)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(&\[&str\]|\[&str;[^\]]*\]|&\[\(&str[^=]*?\]|\[\(&str[^=]*?;[^\]]*\])\s*=\s*&?\[([\s\S]*?)\];/g
  for (const match of source.matchAll(declaration)) {
    tables.push({ name: match[1] ?? '', body: match[3] ?? '', tuples: (match[2] ?? '').includes('(') })
  }
  return tables
}

// The string tables of the gateway that do NOT hold error codes, by file (relative to apps/gateway/src) and then by
// table name, each with its reason. Every other table is read for codes (see emittedCodes), whatever its name, so a
// new table fails the classification test below until it is listed here or it is one of the code tables it names.
// A table is keyed by its file as well as its name, so a table of the same name in another file that does hold codes
// is still read.
const NOT_A_CODE_TABLE: Record<string, Record<string, string>> = {
  'cache_admission.rs': {
    SHAPE_DENY_KEYS: 'JSON keys that keep a response out of the cache, not error codes',
  },
  'domains/actions/handlers.rs': {
    LEGACY_ACTION_ALIASES: 'legacy action names and the current names they map to, not error codes',
  },
  'domains/ag_ui.rs': {
    DEFAULT_FEATURES: 'feature names an AG-UI client may enable, not error codes',
  },
  'domains/agent_actions.rs': {
    PRIVATE_SKILL_FIELDS: 'field names of a private skill body, not error codes',
    AGENT_AUTONOMY_CAPS: 'autonomy cap names of an agent, not error codes',
    AGENT_DRAFT_FIELDS: 'field names of an agent draft, not error codes',
    SPACE_AGENT_EDIT_ROLES: 'Space roles that may edit an agent, not error codes',
    PREVIEW_REFUSED_FIELDS: 'request field names an agent preview refuses, not error codes',
  },
  'domains/ai.rs': {
    DICTATION_CONTEXTS: 'dictation context names, not error codes',
  },
  'domains/audit_compliance.rs': {
    ADMIN_ROLES: 'role names allowed to read the compliance surface, not error codes',
    EVENTS_QUERY: 'query parameter names the audit events route forwards, not error codes',
    MANIFEST_QUERY: 'query parameter names the audit manifest route forwards, not error codes',
    NO_QUERY: 'an empty list of query parameter names a route forwards',
    VERIFY_BODY: 'body field names the audit verify route forwards, not error codes',
  },
  'domains/browser_evidence.rs': {
    DEVTOOLS_LEVELS: 'console levels of a devtools filter, not error codes',
    DEVTOOLS_CATEGORIES: 'event categories of a devtools filter, not error codes',
  },
  'domains/browser_sites.rs': {
    POLICY_MODES: 'policy mode names of a browser site policy, not error codes',
    RULE_KINDS: 'rule kind names of a browser site policy, not error codes',
    CHOICE_LEVELS: 'choice level names of a browser site choice, not error codes',
    POLICY_FIELDS: 'request field names of a browser site policy, not error codes',
    RULE_FIELDS: 'request field names of a browser site rule, not error codes',
    CHOICE_FIELDS: 'request field names of a browser site choice, not error codes',
  },
  'domains/chat/history.rs': {
    RELAYED_CONTEXT_REF_KINDS: 'context reference kinds a chat history relays, not error codes',
  },
  'domains/chat/transcripts.rs': {
    ACCEPTED_AUDIO_TYPES: 'audio media types a transcript accepts, not error codes',
  },
  'domains/client_config.rs': {
    DIAGNOSTIC_KINDS: 'kinds of a client diagnostic beacon, not error codes',
    LONG_TASK_BUCKETS: 'duration buckets of a long-task diagnostic, not error codes',
    CSP_REPORT_CONTENT_TYPES: 'content types a Content-Security-Policy report may be sent as, not error codes',
    CSP_DIRECTIVES: 'Content-Security-Policy directive names, not error codes',
  },
  'domains/ingestions/jobs.rs': {
    CONTROLLABLE_KINDS: 'job kinds that may be controlled, not error codes',
    CONTROL_ACTIONS: 'job control action names, not error codes',
  },
  'domains/ingestions/runs.rs': {
    RUN_KINDS: 'ingestion run kind names, not error codes',
  },
  'domains/ingestions/sources.rs': {
    SOURCE_KINDS: 'ingestion source kind names, not error codes',
  },
  'domains/insights.rs': {
    SURFACES: 'surface names an insights query may name, not error codes',
  },
  'domains/integrations/browse.rs': {
    READ_ONLY_BROWSE_OPS: 'operation names the read-only browse proxy allows, not error codes',
    READ_ONLY_CONTENT_OPS: 'operation names the read-only content proxy allows, not error codes',
  },
  'domains/integrations/credential_bindings.rs': {
    BINDING_FIELDS: 'request field names of a credential binding, not error codes',
  },
  'domains/integrations/model_connections.rs': {
    ZDR_LOCALES: 'languages of a zero-retention acknowledgement text, not error codes',
    ZDR_VARIANTS: 'variants of a zero-retention acknowledgement, not error codes',
    ZDR_ACKNOWLEDGEMENT_FIELDS: 'request field names of a zero-retention acknowledgement, not error codes',
  },
  'domains/knowledge/quarry.rs': {
    PREVIEW_DROPPED_KEYS: 'keys a scrape preview drops from the request, not error codes',
    PREVIEW_DROPPED_FORMATS: 'formats a scrape preview drops from the request, not error codes',
    CRAWL_PAGE_FORMATS: 'page formats a crawl may ask for, not error codes',
  },
  'domains/mcp.rs': {
    MCP_APP_TOOL_CALL_KEYS: 'keys of an MCP app tool call body, not error codes',
  },
  'domains/org_identity.rs': {
    ADMIN_ROLES: 'organisation roles that count as admin, not error codes',
  },
  'domains/orgs/browser_policy.rs': {
    DOCUMENT_FIELDS: 'field names of a browser policy document, not error codes',
  },
  'domains/orgs/model_entitlement.rs': {
    PUT_FIELDS: 'field names of a model entitlement save, not error codes',
    POLICY_FIELDS: 'field names of a model entitlement policy, not error codes',
    SURFACE_DEFAULT_FIELDS: 'field names of a surface default, not error codes',
  },
  'domains/orgs/quotas.rs': {
    ENFORCED_QUOTA_KEYS: 'quota keys the gateway enforces, not error codes',
    VALID_RESET_PERIODS: 'quota reset periods, not error codes',
  },
  'domains/orgs/retention.rs': {
    PUT_FIELDS: 'field names of a retention policy save, not error codes',
    HOLD_FIELDS: 'field names of a legal hold request, not error codes',
    HOLD_READ_ROLES: 'organisation roles that may read legal holds, not error codes',
    HOLD_WRITE_ROLES: 'organisation roles that may place legal holds, not error codes',
  },
  'domains/orgs/settings.rs': {
    ZDR_TIERS: 'zero-retention tiers an organisation may choose, not error codes',
  },
  'domains/orgs/space_policy.rs': {
    PUT_FIELDS: 'field names of a Space policy save, not error codes',
  },
  'domains/persona.rs': {
    TONE_PRESETS: 'tone preset names of a persona, not error codes',
  },
  'domains/spaces/agent_policy.rs': {
    ALLOWED_FIELDS: 'field names an agent policy save may set, not error codes',
    PIN_ALLOWED_FIELDS: 'field names an agent version pin may set, not error codes',
  },
  'domains/spaces/attention.rs': {
    KINDS: 'attention item kinds, not error codes',
    STATES: 'attention item states, not error codes',
    FOLLOW_STATES: 'follow states of a Space thread, not error codes',
    LEVELS: 'notification levels, not error codes',
    READ_ALL_KINDS: 'attention kinds a read-all clears, not error codes',
  },
  'domains/spaces/denial.rs': {
    DETAIL_ALLOWLIST: 'key names of the typed refusal details a Space denial may carry to the browser',
    RESOLVABLE_BY: 'values of the resolvable_by hint of a typed Space denial (who can fix it), not error codes',
  },
  'domains/spaces/files.rs': {
    ALLOWED_EXTENSIONS: 'file extensions a Space upload may carry, not error codes',
  },
  'domains/spaces/message_lifecycle.rs': {
    MODERATION_REASONS: 'moderation reasons a person picks, not error codes',
    REACT_ROLES: 'Space roles that may react to a message, not error codes',
  },
  'domains/spaces/mod.rs': {
    FORGED_SPACE_AUTHORITY_FIELDS: 'request body fields a client may not set',
    FORGED_WAKE_FIELDS: 'request body and frame field names a client may not set on a Space wake, not error codes',
    CREATE_AGENT_ROLES: 'Space roles that may create an agent, not error codes',
    SPACE_INSTRUCTIONS_WRITE_ROLES: 'Space roles that may write the instructions, not error codes',
    MENTIONED_AGENT_FIELDS: 'field names of a mentioned agent reference, not error codes',
    AGENT_PIN_FIELDS: 'field names of an agent pin, not error codes',
    AGENT_ROLES_FIELDS: 'field names of an agent role list, not error codes',
  },
  'domains/spaces/pins.rs': {
    PIN_ROLES: 'Space roles that may pin a post, not error codes',
  },
  'domains/spaces/room_admin.rs': {
    ROOM_ROLES: 'roles a room member may hold, not error codes',
    DETAILS_ROLES: 'roles that may read room details, not error codes',
    VISIBILITIES: 'room visibility values, not error codes',
    JOIN_ROLES: 'roles a joining member may take, not error codes',
    HISTORY_MODES: 'history modes a joining member may take, not error codes',
  },
  'domains/spaces/room_artifacts.rs': {
    STRING_FIELDS: 'JSON field names of an artifact row the room artifacts route keeps',
    NUMBER_FIELDS: 'JSON field names of an artifact row the room artifacts route keeps',
  },
  'domains/spaces/status.rs': {
    STATES: 'presence states of a Space member, not error codes',
  },
  'domains/spaces/stream.rs': {
    RUN_STATES: 'run state names of a Space stream frame, not error codes',
    UPDATED_FIELDS: 'thread field names a thread-updated frame may name, not error codes',
    CLOSING_CODES: 'reasons of a stream close frame; the stream client handles them itself and they never come back as an API error response',
  },
  'domains/spaces/timeline.rs': {
    SYSTEM_ACTIONS: 'system timeline action names, not error codes',
    NOTICE_KINDS: 'notice kinds of the timeline, not error codes',
  },
  'domains/spaces/tool_policy.rs': {
    ALLOWED_FIELDS: 'field names a tool policy save may set, not error codes',
  },
  'main.rs': {
    SENSITIVE_CALLBACK_PREFIXES: 'URL path prefixes of the auth callbacks, not error codes',
  },
  'middleware.rs': {
    STRIPPED_HEADERS: 'header names removed from a proxied request',
  },
  'retention_decision.rs': {
    SPACE_SELECTION_KEYS: 'body keys that name a Space or room, not error codes',
  },
  'upstream.rs': {
    RENDERABLE_ARTIFACT_TYPES: 'media types an artifact may be rendered as, not error codes',
  },
  'zdr_log.rs': {
    SENSITIVE_FIELDS: 'field names the log redaction recognises, not error codes',
    CAPPED_TARGETS: 'log targets with their level caps, not error codes',
  },
}

/** `file:NAME` of every table listed in NOT_A_CODE_TABLE. */
const NOT_A_CODE_TABLE_KEYS: ReadonlySet<string> = new Set(
  Object.entries(NOT_A_CODE_TABLE).flatMap(([file, tables]) => Object.keys(tables).map((name) => `${file}:${name}`)),
)

/** A literal a code table holds: a snake_case-like word, as the gateway names its codes. */
const CODE_SHAPE = /^[A-Za-z][A-Za-z0-9_]+$/

// The shapes a handler names its code in. A code reaches the SPA as
// `{ error: { code, message } }` built by `error("code", ..)`, as a
// (StatusCode, "code", "message") or ("code", "message") pair a helper turns into
// that envelope, as a "code" key of a json! body, as a `code:` field, or through a
// `let code = if/match ..` that picks one of several.
function emittedCodes(file: string, source: string): Set<string> {
  const codes = new Set<string>()
  for (const match of source.matchAll(/\berror\(\s*"([A-Za-z][A-Za-z0-9_]+)"/g)) codes.add(match[1] ?? '')
  for (const match of source.matchAll(/StatusCode::[A-Z_]+,\s*"([a-z][a-z0-9_]+)",\s*"/g)) codes.add(match[1] ?? '')
  for (const match of source.matchAll(/\(\s*"([a-z][a-z0-9_]+)",\s*"[A-Z][^"]* [^"]*"/g)) codes.add(match[1] ?? '')
  for (const match of source.matchAll(/"code":\s*"([A-Za-z][A-Za-z0-9_]+)"/g)) codes.add(match[1] ?? '')
  for (const match of source.matchAll(/\bcode:\s*"([A-Za-z][A-Za-z0-9_]+)"/g)) codes.add(match[1] ?? '')
  for (const block of source.matchAll(/let\s+code\s*=\s*(?:if|match)[\s\S]*?;\n/g)) {
    for (const literal of block[0].matchAll(/"([A-Za-z][A-Za-z0-9_]+)"/g)) codes.add(literal[1] ?? '')
  }
  // A constant table of codes a function returns verbatim: the integration allow-list and the never-plan-gated codes
  // of upstream.rs, the relayed refusals of the BFF (a fixed-size array in retention_decision.rs, tuple tables in
  // orchestration.rs and run_live.rs), the Space stream's codes. Every string table (a slice, a fixed-size array or a
  // table of tuples, any name) is read unless NOT_A_CODE_TABLE classifies it, so a table the scan does not know fails
  // the classification test below. A tuple table's code is its first element; the rest are a status and a message,
  // or another value, and are never read as codes.
  for (const table of stringTables(source)) {
    if (NOT_A_CODE_TABLE_KEYS.has(`${file}:${table.name}`)) continue
    const literals = table.tuples ? table.body.matchAll(/\(\s*"([^"]*)"/g) : table.body.matchAll(/"([^"]*)"/g)
    for (const literal of literals) if (CODE_SHAPE.test(literal[1] ?? '')) codes.add(literal[1] ?? '')
  }
  return codes
}

const emittedBy = new Map<string, string[]>()
for (const [file, source] of sources) {
  for (const code of emittedCodes(file, source)) emittedBy.set(code, [...(emittedBy.get(code) ?? []), file])
}

// Codes the scan cannot see because a helper builds them from a variable
// (`auth_core_error(.., "api_key_error")`, `translation_failure(..)`, the video relabel
// in `search.rs`, the PLAN_REQUIRED of `integration_error_code`). Each one has copy, and
// each literal must still be in the gateway source so the entry cannot rot. The
// constant tables of `upstream.rs` (the integration allow-list and the never-plan-gated
// codes) are not here: `emittedCodes` reads them, so a new entry fails like any other code.
const HELPER_EMITTED = [
  'api_key_error',
  'no_sources_registered',
  'PLAN_REQUIRED',
  'translation_unavailable',
  'translation_invalid_response',
  'video_search_unavailable',
  'video_search_timeout',
] as const

// Helper-built codes that deliberately have no copy, with the reason. The literal must still be in the source.
const HELPER_WITHOUT_COPY: Record<string, string> = {
  integration_error: 'the bounded public message of integration-core is shown on purpose by the Integrations settings (it names the failure of the provider); a generic sentence would hide it',
}

// Codes that have no copy because the one caller words the failure better itself: its fallback names the
// operation, and a code-level sentence would override that (and be wrong for another operation).
const CALLER_WORDS_IT: Record<string, string> = {
  persona_unavailable: 'PersonaSection answers it with "could not save / could not load your persona instructions", which names the operation',
  audit_scope_denied: 'a 403 for a member who may read only their own audit events; the audit client keeps the typed error and the Trust Center draws its own self-scope notice (the 403 status class answers anywhere else)',
  room_admin_unavailable: 'SpaceRoomArchive words it for the archive and restore it belongs to ("Nothing was changed"); a code-level sentence would override that',
  space_policy_admin_disabled: 'a 404 that hides the org Space policy console; OrgSpacePolicySection draws its own "not switched on for this installation" notice',
  browser_policy_admin_disabled: 'a 404 that hides the organization browser policy section; BrowserPolicySection is not mounted unless VITE_BROWSER_POLICY_SETTINGS is on, and a failed load hides it too, so no person sees this code',
  space_room_artifacts_disabled: 'a 404 that turns the room Artifacts tab into its disabled state; SpaceArtifactsTab draws its own notice',
  edit_requires_grant: 'an edited approval refusal (EP-10-S4); ChatMessages words every APPROVAL_EDIT_REFUSALS code through mailEditProblemText, next to the draft it refused',
  edit_requires_once: 'an edited approval refusal (EP-10-S4); ChatMessages words every APPROVAL_EDIT_REFUSALS code through mailEditProblemText, next to the draft it refused',
  edit_too_large: 'an edited approval refusal (EP-10-S4); ChatMessages words every APPROVAL_EDIT_REFUSALS code through mailEditProblemText, which names the message as too long',
  invalid_patch: 'an edited approval refusal (EP-10-S4); ChatMessages words every APPROVAL_EDIT_REFUSALS code through mailEditProblemText, next to the draft it refused',
  invalid_base_fingerprint: 'an edited approval refusal (EP-10-S4); ChatMessages words every APPROVAL_EDIT_REFUSALS code through mailEditProblemText, next to the draft it refused',
}

// Strings the scan meets that are not error codes at all.
const NOT_AN_ERROR_CODE: Record<string, string> = {
  minio: 'name of a dependency in the knowledge diagnostics readiness list, not an error',
  qdrant: 'name of a dependency in the knowledge diagnostics readiness list, not an error',
  quickwit: 'name of a dependency in the knowledge diagnostics readiness list, not an error',
  body: 'the field name of a validation refusal built as a (field, message) pair by the Space policy and room admin helpers; the code sent is the one the helper names',
  expected_revision: 'the field name of a validation refusal built as a (field, message) pair by the room admin helpers',
  idempotency_key: 'the field name of a validation refusal built as a (field, message) pair by the room admin helpers',
  preset_id: 'the field name of a validation refusal built as a (field, message) pair by the Space policy helpers',
  role: 'the field name of a validation refusal built as a (field, message) pair by the room admin helpers',
  approval_id: 'a field name of the Space approval decision body, not an error code',
  decision: 'a field name of the approval and browser evidence bodies, not an error code',
  grant_id: 'a field name of the owner grant and browser grant payloads, not an error code',
  history_mode: 'a field name of the room join request, not an error code',
  join_role: 'a field name of the room join request, not an error code',
  reason: 'a field name of typed refusals and stream frames, not an error code',
  scope: 'a field name of the approval scope and the cache entries, not an error code',
  stable_code: 'a field name of the share redirect body, not an error code',
  visibility: 'a field name of the ticket dispatcher body, not an error code',
}

// Codes that sit inside a 200 payload as `unavailable` sections of a Space. The
// Space panels render their own words for each (the test below checks that), so
// they never go through `translateApiError`.
const SPACE_SECTION_CODES = [
  'approvals_upstream_unavailable',
  'delivery_ledger_not_built',
  'knowledge_binding_unavailable',
  'knowledge_read_not_authorized',
  'knowledge_upstream_unavailable',
  'operations_endpoint_unavailable',
  'operations_upstream_unavailable',
  'processes_read_not_authorized',
  'processes_session_unavailable',
  'processes_upstream_unavailable',
  'runs_read_not_authorized',
  'runs_session_unavailable',
  'runs_upstream_unavailable',
  'schedules_session_unavailable',
  'schedules_upstream_unavailable',
  'watches_not_built',
] as const

// Codes a person cannot cause from the UI, each with the reason. They fall to the
// caller's own sentence (or the generic one) if one ever arrives. A code that GAINS
// copy must be removed from here (checked below), and one the gateway stops
// emitting must be removed too.
const INTERNAL_ONLY: Record<string, string> = {
  attention_stream_limit:
    'the shared attention stream opens in the background and falls back to polling when refused; no person reads this code',
  agent_ref_required: 'the agent picker always sends the agent reference; only a hand-made request omits it',
  binding_ref_required: 'the agent row always carries its binding reference; only a hand-made request omits it',
  ambiguous_space_selection: 'the SPA sends one Space reference; two conflicting ones is a client bug',
  invalid_action_input: 'the action runner sends an object; a non-object body is a client bug',
  invalid_agent_binding_status: 'the SPA only offers pause and resume; any other status is a hand-made request',
  invalid_agent_input: 'the agent-UI stream body is built by the client; the message names the missing field for developers',
  invalid_ai_text_proposal: 'proposal ids are generated by the client; a malformed one is a client bug',
  invalid_api_key_id: 'the key id comes from the listed key a person clicks; an empty one is a client bug',
  invalid_chat_request: 'the Space chat body is built by the client; a non-object is a client bug',
  invalid_cost_scope: 'the usage scope is a fixed toggle in the SPA; any other value is a hand-made request',
  invalid_deletion_request: 'the Space reference and idempotency key are generated by the client',
  invalid_import_request: 'the import source type and key are chosen and generated by the client',
  invalid_insights_query: 'the insight filters are picked from fixed lists in the SPA',
  invalid_owner_grant_request: 'the grant selection and idempotency key are generated by the client',
  invalid_presence: 'the SPA only sends online, typing or offline',
  invalid_space: 'the Space reference comes from the route the person opened; an empty one is a client bug',
  operation_not_allowed: 'the integration browse operation is chosen by the SPA from a fixed list',
  operation_required: 'the integration browse operation is always sent by the SPA',
  speech_provider_not_allowed: 'the SPA does not offer another text-to-speech provider',
  unsupported_space_kind: 'the SPA only creates a personal Space or a room',
  invalid_diagnostic: 'the diagnostics beacon is fire-and-forget; no screen shows its refusal',
  invalid_report: 'the Content-Security-Policy report endpoint is called by the browser, not by a screen',
  report_too_large: 'the Content-Security-Policy report endpoint is called by the browser, not by a screen',
  space_role_insufficient: 'Control Plane names it as the reason of a Space denial; the gateway sends it inside details.reason of a typed refusal and the denial presenter words it',
  mail_task_pipeline_failed: 'the mail-task ingest endpoint is called service to service; no SPA code calls it',
  mail_task_pipeline_not_configured: 'the mail-task ingest endpoint is called service to service; no SPA code calls it',
  capabilities_unavailable: 'the composer capabilities read (EP-3 S8) runs in the background; getComposerCapabilities reads any failure as nothing offered and no person reads the code',
  invalid_origin: 'the promote-to-document body (EP-3 S8) is built by the SPA with a fixed origin; any other origin is a hand-made request',
  invalid_source_ref: 'the promote-to-document source reference (EP-3 S8) is built by the SPA from the artifact on screen; a malformed one is a client bug',
}

// A model-entitlement refusal reads its copy from the model-entitlement catalogue, which
// `translateApiError` checks before ERROR_COPY, so that catalogue counts as copy here. A
// zero-retention refusal reads its copy from the zero-retention catalogue the same way.
function hasCopy(code: string): boolean {
  return Boolean(ERROR_COPY[code] ?? BLOCK_REASON_COPY[code]) || isModelEntitlementCode(code) || isZdrRefusalCode(code)
}

describe('every code the gateway emits', () => {
  it('has found the gateway sources and a plausible number of codes', () => {
    expect(sources.size).toBeGreaterThan(100)
    expect(emittedBy.size).toBeGreaterThan(200)
    // The scan sees the codes of the middleware, the proxy layer, onboarding and a domain.
    for (const [code, file] of [
      ['unauthorized', 'middleware.rs'],
      ['session_verification_unavailable', 'middleware.rs'],
      ['upstream_error', 'upstream.rs'],
      ['invalid_name', 'onboarding/actions/organization.rs'],
      ['space_not_found', 'domains/spaces/mod.rs'],
      ['invalid_url', 'domains/browser.rs'],
      ['scrape_failed', 'domains/knowledge/quarry.rs'],
      ['model_error', 'upstream.rs'],
      // Read from the constant tables of integration_error_code, not from a call.
      ['personal_connection_forbidden', 'upstream.rs'],
      ['personal_connection_no_delegated_user', 'upstream.rs'],
      ['no_sources_registered', 'upstream.rs'],
      // Read from a fixed-size array (DECISION_REFUSAL_CODES) and from the first element of multi-line tuple tables.
      ['zdr_waiver_route_mismatch', 'retention_decision.rs'],
      ['continue_disabled', 'domains/orchestration.rs'],
      ['too_many_live_views', 'domains/spaces/run_live.rs'],
    ] as const) {
      expect(emittedBy.get(code), `the scan found ${code}`).toContain(file)
    }
  })

  it('reads the test modules a #[cfg(test)] declaration names as tests, not as production code', () => {
    // The `#[path]` form (zdr.rs) and the plain form (domains/chat.rs names domains/chat/bff_turn_tests.rs).
    for (const file of ['zdr_posture_matrix_tests.rs', 'domains/chat/bff_turn_tests.rs']) {
      expect(testOnly.has(resolve(gatewaySrc, file)), `${file} is declared under cfg(test)`).toBe(true)
      expect(sources.has(file), `${file} is not production code`).toBe(false)
    }
    expect(sources.has('zdr.rs'), 'the production file that declares a test module is still scanned').toBe(true)
  })

  it('classifies every string table of the gateway: read for codes, or listed as not holding codes', () => {
    const read: string[] = []
    const seen = new Set<string>()
    for (const [file, source] of sources) {
      for (const table of stringTables(source)) {
        const key = `${file}:${table.name}`
        seen.add(key)
        if (!NOT_A_CODE_TABLE_KEYS.has(key)) read.push(key)
      }
    }
    // The code tables: the relayed refusals of the BFF (DECISION_REFUSAL_CODES is a fixed-size array, CONTINUE_RELAYED_ERRORS,
    // RELAYED and STOP_RELAYED are tables of tuples), the Space stream's codes, the upstream allow-lists and the scrape block
    // reasons. A new table lands here and has to be added to this list (it holds codes) or to NOT_A_CODE_TABLE (it does not).
    expect(read.sort()).toEqual([
      'domains/knowledge/quarry.rs:SCRAPE_BLOCK_REASONS',
      'domains/orchestration.rs:CONTINUE_RELAYED_ERRORS',
      'domains/spaces/run_live.rs:RELAYED',
      'domains/spaces/run_live.rs:STOP_RELAYED',
      'domains/spaces/stream.rs:ERROR_CODES',
      'retention_decision.rs:DECISION_REFUSAL_CODES',
      'upstream.rs:INTEGRATION_ERROR_CODE_ALLOWLIST',
      'upstream.rs:NEVER_PLAN_GATED_INTEGRATION_ERROR_CODES',
    ])
    for (const key of NOT_A_CODE_TABLE_KEYS) {
      expect(seen.has(key), `${key} is no longer a table in the gateway, remove it from NOT_A_CODE_TABLE`).toBe(true)
    }
    for (const [file, tables] of Object.entries(NOT_A_CODE_TABLE)) {
      for (const [name, reason] of Object.entries(tables)) expect(reason.length, `${file}:${name} names its reason`).toBeGreaterThan(20)
    }
  })

  it('has copy, or an explicit reason it needs none', () => {
    const exempt = new Set([
      ...Object.keys(INTERNAL_ONLY),
      ...Object.keys(CALLER_WORDS_IT),
      ...Object.keys(NOT_AN_ERROR_CODE),
      ...SPACE_SECTION_CODES,
    ])
    const without = [...emittedBy.keys()].filter((code) => !hasCopy(code) && !exempt.has(code)).sort()
    expect(without, 'a code the gateway emits has no copy in errors.ts and no entry in an exemption list of this test').toEqual([])
  })

  it('keeps the exemption lists honest: each entry is still emitted and still has no copy', () => {
    const lists: Record<string, string[]> = {
      INTERNAL_ONLY: Object.keys(INTERNAL_ONLY),
      CALLER_WORDS_IT: Object.keys(CALLER_WORDS_IT),
      NOT_AN_ERROR_CODE: Object.keys(NOT_AN_ERROR_CODE),
      SPACE_SECTION_CODES: [...SPACE_SECTION_CODES],
    }
    for (const [name, codes] of Object.entries(lists)) {
      for (const code of codes) {
        expect(emittedBy.has(code), `${name}: ${code} is no longer emitted, remove it`).toBe(true)
        expect(hasCopy(code), `${name}: ${code} has copy now, remove it`).toBe(false)
      }
    }
    for (const reason of [
      ...Object.values(INTERNAL_ONLY),
      ...Object.values(NOT_AN_ERROR_CODE),
      ...Object.values(CALLER_WORDS_IT),
      ...Object.values(HELPER_WITHOUT_COPY),
    ]) {
      expect(reason.length, 'every exemption names its reason').toBeGreaterThan(20)
    }
  })

  it('has the copy of each helper-built code, and the helper still names it', () => {
    const everything = [...sources.values()].join('\n')
    for (const code of HELPER_EMITTED) {
      expect(everything, `${code} is still in the gateway source`).toContain(`"${code}"`)
      expect(ERROR_COPY[code], `${code} has copy`).toBeDefined()
    }
    for (const code of Object.keys(HELPER_WITHOUT_COPY)) {
      expect(everything, `${code} is still in the gateway source`).toContain(`"${code}"`)
      expect(hasCopy(code), `${code} has copy now, move it to HELPER_EMITTED`).toBe(false)
    }
  })

  it('shows the Space section codes in the Space panels, in words of their own', () => {
    const dir = resolve(process.cwd(), 'src/features/spaces/components')
    const panels = readdirSync(dir)
      .filter((name) => name.endsWith('.tsx') && !name.endsWith('.test.tsx'))
      .map((name) => readFileSync(resolve(dir, name), 'utf8'))
      .join('\n')
    for (const code of SPACE_SECTION_CODES) expect(panels, `${code} has a case in a Space panel`).toContain(`'${code}'`)
  })

  // WS5.G: the gateway answers a 415 from quarry-edge with `unsupported_media`, and the
  // copy for it is in the same tree. It is held to "emitted means copied" like every code;
  // this pins that the scan sees it and that the copy reads as a sentence in both languages.
  it('has copy for the 415 code the scrape preview answers with', () => {
    expect(emittedBy.get('unsupported_media'), 'the scan found unsupported_media').toContain('domains/knowledge/quarry.rs')
    const copy = ERROR_COPY.unsupported_media
    expect(copy).toBeDefined()
    expect(copy?.no.length).toBeGreaterThan(15)
    expect(copy?.en.length).toBeGreaterThan(15)
  })
})

// What Quarry answers a browser call with and the gateway passes through unchanged
// (the closed `ErrorCode` set of `quarry-core`). The gateway maps the first two to
// its own codes; the generic ones are answered by the status class.
const QUARRY_PASSTHROUGH = {
  mappedByGateway: ['CHALLENGE_DETECTED', 'USER_TOOK_OVER'],
  copy: [
    'RATE_LIMITED',
    'TIMEOUT',
    'SECURITY_BLOCKED',
    'DRIVER_FAILED',
    'RUNTIME_NOT_READY',
    'CHECKPOINT_LOST',
    'TARGET_REPAIR_REQUIRED',
    'UPSTREAM_BLOCKED',
    'UNSUPPORTED',
    'UNSUPPORTED_MEDIA',
    'BUDGET_EXHAUSTED',
  ],
  byStatusClass: ['BAD_REQUEST', 'UNAUTHORIZED', 'FORBIDDEN', 'NOT_FOUND', 'CONFLICT', 'ACTION_UNKNOWN', 'INTERNAL'],
} as const

// The codes added to errors.ts for this list. Each has to read as copy for the
// person at the screen: no raw id, no internal word, a full sentence, two languages.
const ADDED_CODES = [
  'space_not_found',
  'space_read_not_authorized',
  'space_lifecycle_unavailable',
  'space_deletion_unavailable',
  'space_deletion_not_found',
  'space_provisioning_unavailable',
  'space_required',
  'space_role_cannot_create_agent',
  'space_role_cannot_edit_instructions',
  'room_name_required',
  'room_membership_unavailable',
  'room_membership_unconfirmed',
  'member_required',
  'member_not_found',
  'thread_presentation_owner_only',
  'thread_presentation_unavailable',
  'invalid_presentation',
  'presence_unavailable',
  'read_marker_unavailable',
  'scoped_append_content_required',
  'active_membership_required',
  'owner_grant_role_required',
  'invalid_owner_grant_decision',
  'invalid_space_decision',
  'owner_action_contract_invalid',
  'ticket_operation_receipt_invalid',
  'invalid_model_gateway_response',
  'agent_binding_state_unavailable',
  'agent_binding_unavailable',
  'agent_creation_unavailable',
  'agent_invocation_blocked',
  'agent_membership_unconfirmed',
  'agent_mention_not_enabled',
  'agent_mention_requires_space',
  'mentioned_agent_not_bound',
  'agent_registry_unavailable',
  'agent_revocation_unconfirmed',
  'installable_agents_unavailable',
  'invalid_agent_instructions',
  'invalid_agent_name',
  'instructions_too_long',
  'instructions_unavailable',
  'persona_instructions_too_long',
  'invalid_cron_request',
  'invalid_task_template',
  'task_template_required',
  'active_organization_required',
  'ai_review_mode_required',
  'zdr_ai_proposal_forbidden',
  'zdr_draft_persistence_forbidden',
  'zdr_recurrence_forbidden',
  'support_ai_policy_unavailable',
  'retention_posture_unavailable',
  'support_recurrence_permission_required',
  'support_recurrence_policy_unavailable',
  'thread_not_found',
  'sharepoint_sites_unavailable',
  'sharepoint_drives_unavailable',
  'sharepoint_folders_unavailable',
  'site_id_required',
  'drive_id_required',
  'invalid_content',
  'invalid_body',
  'no_products',
  'summary_failed',
  'invalid_query',
  'web_search_unavailable',
  'web_search_timeout',
  'video_search_timeout',
  'insights_upstream_unavailable',
  'artifact_unavailable',
  'artifact_too_large',
  'connection_not_found',
  'invalid_name',
  'onboarding_completion_failed',
  'recommendation_unavailable',
  'translation_unavailable',
  'translation_invalid_response',
  'empty_crawl',
  'control_unreachable',
  'crawl_failed',
  'invalid_checkout_request',
  'session_verification_unavailable',
  'api_key_error',
  'invalid_api_key_name',
  'invalid_color_scheme',
  'invalid_theme',
  'invalid_request',
  'no_sources_registered',
  'PLAN_REQUIRED',
  'model_error',
  'model_request_timeout',
  'INVITATION_AUTHORITY_UNAVAILABLE',
  'TOO_MANY_REQUESTS',
  'USER_ALREADY_EXISTS',
  'DISPOSABLE_EMAIL_BLOCKED',
  'RATE_LIMITED',
  'TIMEOUT',
  'SECURITY_BLOCKED',
  'DRIVER_FAILED',
  'RUNTIME_NOT_READY',
  'CHECKPOINT_LOST',
  'TARGET_REPAIR_REQUIRED',
  'UPSTREAM_BLOCKED',
  'UNSUPPORTED',
  'UNSUPPORTED_MEDIA',
  'BUDGET_EXHAUSTED',
  // W3-E3.8.5: the fetch reasons of a failed scrape (gateway knowledge/shared.rs).
  'scrape_host_not_found',
  'scrape_unreachable',
  'scrape_tls_failed',
  // The relayed refusals of an approval or question continuation (CONTINUE_RELAYED_ERRORS) and of a live view (RELAYED).
  'continue_disabled',
  'not_resumable',
  'approval_not_granted',
  'approval_not_resumable',
  'continuation_already_consumed',
  'continue_unavailable',
  'question_not_found',
  'question_expired',
  'question_already_answered',
  'invalid_answer',
  'question_not_resumable',
  'run_stream_expired',
  'too_many_live_views',
  'run_live_unavailable',
  // The chat stream's mode_lockdown (model-gateway's refusal of an agentic turn) and the catalogue's three refusals the BFF relays.
  'mode_lockdown',
  'zdr_connection_not_acknowledged',
  'zdr_posture_mismatch',
  'zdr_waiver_route_mismatch',
] as readonly string[]

// An internal id in a sentence: snake_case or UPPER_SNAKE words, plus the names
// of services and mechanisms a person has no use for.
const RAW_ID = /\b[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b|\b[A-Z]{2,}(?:_[A-Z0-9]+)+\b/
const INTERNAL_WORD = /quarry|hbbs|hbbr|grant|gateway|model plane|control plane|json|https?:|\b[45]\d\d\b/i

describe('the copy for the codes of the whole gateway', () => {
  it.each(ADDED_CODES)('reads as end-user copy for %s', (code) => {
    const copy = ERROR_COPY[code]
    expect(copy, `${code} has copy`).toBeDefined()
    for (const text of [copy!.no, copy!.en]) {
      expect(text.trim().length, `${code}: long enough to say something`).toBeGreaterThan(15)
      expect(text, `${code}: no raw id`).not.toMatch(RAW_ID)
      expect(text, `${code}: no internal word`).not.toMatch(INTERNAL_WORD)
      expect(text.endsWith('.'), `${code}: ends as a sentence`).toBe(true)
    }
    expect(copy!.no, `${code}: the two languages differ`).not.toBe(copy!.en)
  })

  it('covers every Quarry code a browser call can pass through', () => {
    for (const code of QUARRY_PASSTHROUGH.copy) expect(ERROR_COPY[code], code).toBeDefined()
    for (const code of QUARRY_PASSTHROUGH.mappedByGateway) expect(ERROR_COPY[code], code).toBeDefined()
    // The generic ones are answered by status, not by a sentence of their own.
    for (const code of QUARRY_PASSTHROUGH.byStatusClass) expect(ERROR_COPY[code], `${code} is answered by status`).toBeUndefined()
    expect(describeApiError({ code: 'NOT_FOUND', status: 404 }, no, { no: 'Fant ikke økten.', en: 'x' }).message).toBe('Fant ikke økten.')
    expect(describeApiError({ code: 'UNAUTHORIZED', status: 401 }, no).message).toBe('Du må logge inn på nytt.')
    expect(describeApiError({ code: 'FORBIDDEN', status: 403 }, no).kind).toBe('forbidden')
  })

  it('names the rooms and agents in words a person uses, in both languages', () => {
    expect(translateApiError({ code: 'space_not_found', status: 404 }, no)).toMatch(/Fant ikke dette rommet/)
    expect(translateApiError({ code: 'space_not_found', status: 404 }, en)).toMatch(/could not be found/)
    expect(translateApiError({ code: 'zdr_draft_persistence_forbidden', status: 412 }, en)).toMatch(/Zero Data Retention/)
    expect(translateApiError({ code: 'session_verification_unavailable', status: 503 }, no)).toMatch(/økten er ikke endret/)
  })
})

describe('translateApiError answers by status class like describeApiError', () => {
  const failure = (fields: Record<string, unknown>) => ({ code: null, status: 400, ...fields })

  it('reports a lost connection as one, not as the caller\'s "could not do X"', () => {
    const fallback = { no: 'Kunne ikke lagre.', en: 'Could not save.' }
    expect(translateApiError(new TypeError('Failed to fetch'), no, fallback)).toMatch(/Fikk ikke kontakt/)
    expect(translateApiError(new TypeError('Load failed'), en, fallback)).toMatch(/Could not reach the service/)
    expect(translateApiError(new TypeError('Failed to fetch'), no, fallback)).not.toMatch(/Failed to fetch|Load failed/)
  })

  it('shows the wait of a 429 that carries no code of its own', () => {
    const fallback = { no: 'Kunne ikke lagre.', en: 'Could not save.' }
    expect(translateApiError(failure({ status: 429, retryAfterSeconds: 12 }), no, fallback)).toBe(
      'Det kommer for mange forespørsler akkurat nå. Prøv igjen om 12 sekunder.',
    )
    expect(translateApiError(failure({ status: 429 }), en, fallback)).toBe('There are too many requests right now. Try again shortly.')
  })

  it('tells a refusal and an expired session from a failed save', () => {
    const fallback = { no: 'Kunne ikke lagre.', en: 'Could not save.' }
    expect(translateApiError(failure({ status: 403 }), no, fallback)).toBe('Du har ikke tilgang til å gjøre dette.')
    expect(translateApiError(failure({ status: 401 }), no, fallback)).toBe('Du må logge inn på nytt.')
  })

  it('leaves 404, 409, 422 and 5xx to the caller\'s own sentence, which names what failed', () => {
    const fallback = { no: 'Kunne ikke lagre.', en: 'Could not save.' }
    for (const status of [404, 409, 422, 500, 502, 503]) {
      expect(translateApiError(failure({ status }), no, fallback), String(status)).toBe('Kunne ikke lagre.')
    }
    // ...unless the code has copy of its own.
    expect(translateApiError(failure({ status: 422, code: 'validation_error' }), no, fallback)).toMatch(/Noen av feltene/)
    expect(translateApiError(failure({ status: 503, code: 'upstream_unavailable' }), no, fallback)).toMatch(/utilgjengelig/)
  })

  it('never shows the raw gateway message', () => {
    const err = Object.assign(new Error('Request failed (500): postgres://leak'), { status: 500, code: 'something_new' })
    expect(translateApiError(err, no)).not.toMatch(/Request failed|postgres/)
  })
})
