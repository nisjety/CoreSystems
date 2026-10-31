# AgentApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**agentInteract**](AgentApi.md#agentinteract) | **POST** /v1/agent/runs/{run_id}/interact | Ergonomic alias for one governed browser action |
| [**agentStep**](AgentApi.md#agentstepoperation) | **POST** /v1/agent/runs/{run_id}/step | Execute one governed browser action |
| [**attachAgentRun**](AgentApi.md#attachagentrunoperation) | **POST** /v1/agent/runs/{run_id}/attach | Attach an agent to a person\&#39;s live run (dark; service principal only) |
| [**checkAgentProcedureReplay**](AgentApi.md#checkagentprocedurereplay) | **POST** /v1/agent/procedures/replay-check | Compare actions with a procedure without executing effects |
| [**closeAgentRun**](AgentApi.md#closeagentrun) | **DELETE** /v1/agent/runs/{run_id} | Close a browser-agent run and release its lease |
| [**closeAgentTab**](AgentApi.md#closeagenttab) | **DELETE** /v1/agent/runs/{run_id}/tabs/{tab_id} | Close a tab |
| [**compileAgentProcedure**](AgentApi.md#compileagentprocedure) | **POST** /v1/agent/runs/{run_id}/procedure | Compile verified receipts into a replay candidate |
| [**impactCheckAgentProcedure**](AgentApi.md#impactcheckagentprocedure) | **POST** /v1/agent/procedures/impact-check | Identify changed sources that require procedure quarantine |
| [**listAgentDevtoolsEvents**](AgentApi.md#listagentdevtoolsevents) | **GET** /v1/agent/runs/{run_id}/devtools | Read a filtered window of transient browser DevTools events |
| [**listAgentEgressReceipts**](AgentApi.md#listagentegressreceipts) | **GET** /v1/agent/runs/{run_id}/egress-receipts | Read redacted live browser egress decisions |
| [**listAgentReceipts**](AgentApi.md#listagentreceipts) | **GET** /v1/agent/runs/{run_id}/receipts | Read tenant-scoped immutable action receipts |
| [**listAgentTabs**](AgentApi.md#listagenttabs) | **GET** /v1/agent/runs/{run_id}/tabs | List the live browser tabs of a run |
| [**openAgentTab**](AgentApi.md#openagenttaboperation) | **POST** /v1/agent/runs/{run_id}/tabs | Open a browser tab on a run |
| [**qualityCheckAgentProcedure**](AgentApi.md#qualitycheckagentprocedure) | **POST** /v1/agent/procedures/quality-check | Evaluate deterministic procedure promotion evidence |
| [**releaseAgentAttach**](AgentApi.md#releaseagentattach) | **DELETE** /v1/agent/runs/{run_id}/attach/{attach_id} | End an agent attachment (the delegate ends its own; the owner stops it) |
| [**selectAgentTab**](AgentApi.md#selectagenttab) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/select | Make a tab the active one |
| [**setAgentTabHolder**](AgentApi.md#setagenttabholderoperation) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/control | Hand a tab to the agent or take it back (the owner\&#39;s own token only) |
| [**startAgentRun**](AgentApi.md#startagentrunoperation) | **POST** /v1/agent/runs | Acquire a governed browser-agent run |
| [**streamAgentRunEvents**](AgentApi.md#streamagentrunevents) | **GET** /v1/agent/runs/{run_id}/events/stream | Typed agent-run events as Server-Sent Events, with resume (WS10.S3) |



## agentInteract

> EnvelopeBrowserObservation agentInteract(runId, agentStepRequest)

Ergonomic alias for one governed browser action

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { AgentInteractRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // AgentStepRequest
    agentStepRequest: ...,
  } satisfies AgentInteractRequest;

  try {
    const data = await api.agentInteract(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **agentStepRequest** | [AgentStepRequest](AgentStepRequest.md) |  | |

### Return type

[**EnvelopeBrowserObservation**](EnvelopeBrowserObservation.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Observation and immutable outcome proof |  -  |
| **403** | Domain, SSRF, policy, or grant denial |  -  |
| **409** | Snapshot target is stale, absent, ambiguous, or changed; re-observe before acting |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## agentStep

> EnvelopeBrowserObservation agentStep(runId, agentStepRequest)

Execute one governed browser action

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { AgentStepOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // AgentStepRequest
    agentStepRequest: ...,
  } satisfies AgentStepOperationRequest;

  try {
    const data = await api.agentStep(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **agentStepRequest** | [AgentStepRequest](AgentStepRequest.md) |  | |

### Return type

[**EnvelopeBrowserObservation**](EnvelopeBrowserObservation.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Observation and immutable outcome proof |  -  |
| **403** | Domain, SSRF, policy, or grant denial |  -  |
| **409** | Either the snapshot target is stale, absent, ambiguous, or changed (re-observe before acting), or USER_TOOK_OVER: the person using the browser has taken it over and this action was proposed by an agent (a service principal, or &#x60;initiated_by: agent&#x60;). &#x60;error.details&#x60; is &#x60;{control_mode: \&quot;human_takeover\&quot;, control_epoch, taken_over_at}&#x60; (&#x60;taken_over_at&#x60;, RFC 3339, is absent for a run resumed into a takeover). Not retryable until control is handed back. Enforced only when QUARRY_EDGE__TAKEOVER_ENFORCEMENT&#x3D;1; otherwise the edge logs what it would have refused. |  -  |
| **402** | BUDGET_EXHAUSTED. An agent run reached its step, attempt, runtime or estimated-cost bound (error.details.dimension is steps, attempts, runtime or cost, with limit, spent and unit). Not retryable: the budget does not refill, and a resumed run keeps what it spent. Only returned when QUARRY_EDGE__BUDGET_ENFORCEMENT&#x3D;on; in shadow mode the same condition is only recorded. |  -  |
| **429** | Step budget exhausted (the step limit the request itself set) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## attachAgentRun

> AttachAgentRun200Response attachAgentRun(runId, attachAgentRunRequest)

Attach an agent to a person\&#39;s live run (dark; service principal only)

WS8.F. Off unless QUARRY_EDGE__ATTACH is on. A service principal holding an attach grant (a child of the run\&#39;s grant, for this run, owned by the run\&#39;s owner, one to eight hosts) gets one agent tab, opened in the background: the person\&#39;s active tab does not change. Refused for ZDR runs. Refusal reasons (in error.message): attach_disabled, attach_requires_service_principal, attach_zdr_refused, attach_requires_grant, attach_grant_invalid, attach_driver_unsupported (UNSUPPORTED), attach_exists (CONFLICT).

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { AttachAgentRunOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // AttachAgentRunRequest
    attachAgentRunRequest: ...,
  } satisfies AttachAgentRunOperationRequest;

  try {
    const data = await api.attachAgentRun(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **attachAgentRunRequest** | [AttachAgentRunRequest](AttachAgentRunRequest.md) |  | |

### Return type

[**AttachAgentRun200Response**](AttachAgentRun200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The attachment and its agent tab |  -  |
| **400** | A constraint is outside its bounds |  -  |
| **403** | Refused (the reason is in error.message) |  -  |
| **404** | Run not found |  -  |
| **409** | An attachment is already live on the run |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## checkAgentProcedureReplay

> EnvelopeReplayDecision checkAgentProcedureReplay(replayCheckRequest)

Compare actions with a procedure without executing effects

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CheckAgentProcedureReplayRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // ReplayCheckRequest
    replayCheckRequest: ...,
  } satisfies CheckAgentProcedureReplayRequest;

  try {
    const data = await api.checkAgentProcedureReplay(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **replayCheckRequest** | [ReplayCheckRequest](ReplayCheckRequest.md) |  | |

### Return type

[**EnvelopeReplayDecision**](EnvelopeReplayDecision.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Exact, repair-required, or refused decision |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## closeAgentRun

> EnvelopeClosedRun closeAgentRun(runId)

Close a browser-agent run and release its lease

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CloseAgentRunRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies CloseAgentRunRequest;

  try {
    const data = await api.closeAgentRun(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**EnvelopeClosedRun**](EnvelopeClosedRun.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Run closed |  -  |
| **404** | Run not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## closeAgentTab

> EnvelopeAgentTabs closeAgentTab(runId, tabId, initiatedBy, aiRunId, agentLabel)

Close a tab

Has no body, so the hint and the advisory fields are query parameters. Same take-over rule and recording as opening a tab; the tab\&#39;s opener record is dropped.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CloseAgentTabRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | A tab id from the run\'s tab list
    tabId: tabId_example,
    // 'human' | 'agent' | Who is making the call. Can only tighten: `agent` on a person\'s token makes it an agent\'s, and a service principal is always an agent. `initiated_by` is accepted too (both in one request is an error). (optional)
    initiatedBy: initiatedBy_example,
    // string | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. `ai_run_id` is accepted too. (optional)
    aiRunId: aiRunId_example,
    // string | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. `agent_label` is accepted too. (optional)
    agentLabel: agentLabel_example,
  } satisfies CloseAgentTabRequest;

  try {
    const data = await api.closeAgentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | A tab id from the run\&#39;s tab list | [Defaults to `undefined`] |
| **initiatedBy** | `human`, `agent` | Who is making the call. Can only tighten: &#x60;agent&#x60; on a person\&#39;s token makes it an agent\&#39;s, and a service principal is always an agent. &#x60;initiated_by&#x60; is accepted too (both in one request is an error). | [Optional] [Defaults to `undefined`] [Enum: human, agent] |
| **aiRunId** | `string` | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. &#x60;ai_run_id&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |
| **agentLabel** | `string` | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. &#x60;agent_label&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |

### Return type

[**EnvelopeAgentTabs**](EnvelopeAgentTabs.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The run\&#39;s tabs |  -  |
| **400** | A hint that is not &#x60;human&#x60; or &#x60;agent&#x60; |  -  |
| **404** | Run not found, not owned by the caller, or no longer live |  -  |
| **409** | USER_TOOK_OVER, as for opening a tab |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## compileAgentProcedure

> EnvelopeBrowserProcedure compileAgentProcedure(runId, compileProcedureRequest)

Compile verified receipts into a replay candidate

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CompileAgentProcedureRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // CompileProcedureRequest
    compileProcedureRequest: ...,
  } satisfies CompileAgentProcedureRequest;

  try {
    const data = await api.compileAgentProcedure(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **compileProcedureRequest** | [CompileProcedureRequest](CompileProcedureRequest.md) |  | |

### Return type

[**EnvelopeBrowserProcedure**](EnvelopeBrowserProcedure.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Deterministic procedure candidate |  -  |
| **409** | Receipt stream contains unverified or incomplete effects |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## impactCheckAgentProcedure

> EnvelopeProcedureImpact impactCheckAgentProcedure(procedureImpactRequest)

Identify changed sources that require procedure quarantine

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ImpactCheckAgentProcedureRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // ProcedureImpactRequest
    procedureImpactRequest: ...,
  } satisfies ImpactCheckAgentProcedureRequest;

  try {
    const data = await api.impactCheckAgentProcedure(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **procedureImpactRequest** | [ProcedureImpactRequest](ProcedureImpactRequest.md) |  | |

### Return type

[**EnvelopeProcedureImpact**](EnvelopeProcedureImpact.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Conservative change-impact report |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentDevtoolsEvents

> ListAgentDevtoolsEvents200Response listAgentDevtoolsEvents(runId, afterSequence, limit, levels, filter, category, tabId)

Read a filtered window of transient browser DevTools events

Returns events newer than afterSequence from the driver\&#39;s bounded buffer (512 events). Filters are optional and combine with AND. levels accepts debug, info, log, warn, warning, error and verbose (at most five; warn is warning, log is info, verbose is debug). filter is a case-insensitive substring over text, url and name only, never payload, at most 200 characters without control characters. nextSequence is the last event examined; pass it back as afterSequence to continue even when no event matched.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentDevtoolsEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // number (optional)
    afterSequence: 56,
    // number (optional)
    limit: 56,
    // string (optional)
    levels: warn,error,
    // string (optional)
    filter: filter_example,
    // 'console' | 'network' | 'lifecycle' (optional)
    category: category_example,
    // string (optional)
    tabId: tabId_example,
  } satisfies ListAgentDevtoolsEventsRequest;

  try {
    const data = await api.listAgentDevtoolsEvents(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **afterSequence** | `number` |  | [Optional] [Defaults to `0`] |
| **limit** | `number` |  | [Optional] [Defaults to `100`] |
| **levels** | `string` |  | [Optional] [Defaults to `undefined`] |
| **filter** | `string` |  | [Optional] [Defaults to `undefined`] |
| **category** | `console`, `network`, `lifecycle` |  | [Optional] [Defaults to `undefined`] [Enum: console, network, lifecycle] |
| **tabId** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

[**ListAgentDevtoolsEvents200Response**](ListAgentDevtoolsEvents200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Matching events in sequence order, with the resume cursor and ZDR flag |  -  |
| **400** | A filter value is outside the documented allowlist |  -  |
| **404** | Run not found or not owned by the caller |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentEgressReceipts

> EnvelopeEgressReceipts listAgentEgressReceipts(runId, afterSequence, limit)

Read redacted live browser egress decisions

Returns bounded, driver-authored policy receipts for the live session. URLs contain only origin and path; query strings, headers, bodies, credentials, resolved IP addresses, and proxy details are omitted.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentEgressReceiptsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // number (optional)
    afterSequence: 56,
    // number (optional)
    limit: 56,
  } satisfies ListAgentEgressReceiptsRequest;

  try {
    const data = await api.listAgentEgressReceipts(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **afterSequence** | `number` |  | [Optional] [Defaults to `0`] |
| **limit** | `number` |  | [Optional] [Defaults to `100`] |

### Return type

[**EnvelopeEgressReceipts**](EnvelopeEgressReceipts.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Ordered, redacted egress decisions |  -  |
| **404** | Run not found or not owned by the caller |  -  |
| **409** | Requested receipt cursor is no longer continuous in the bounded proof buffer |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentReceipts

> EnvelopeStepReceipts listAgentReceipts(runId)

Read tenant-scoped immutable action receipts

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentReceiptsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies ListAgentReceiptsRequest;

  try {
    const data = await api.listAgentReceipts(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**EnvelopeStepReceipts**](EnvelopeStepReceipts.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Ordered action receipt stream |  -  |
| **404** | Run or receipt stream not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentTabs

> EnvelopeAgentTabs listAgentTabs(runId)

List the live browser tabs of a run

The driver\&#39;s current tabs for the run. A tab carries no opener here; the owner projection (&#x60;GET .../browser-session&#x60;, see CONTRACTS.md) names who opened each tab as &#x60;openedBy&#x60;.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentTabsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies ListAgentTabsRequest;

  try {
    const data = await api.listAgentTabs(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**EnvelopeAgentTabs**](EnvelopeAgentTabs.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The run\&#39;s tabs |  -  |
| **404** | Run not found or not owned by the caller |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## openAgentTab

> EnvelopeAgentTab openAgentTab(runId, openAgentTabRequest)

Open a browser tab on a run

Opens a tab (optionally navigated to &#x60;url&#x60;) and records who did it. Subject to the same gates as a step: the run\&#39;s domains, its grant, and the take-over rule. Who is calling comes from the token (a service principal is an agent whatever it sends; a person is a person unless &#x60;initiatedBy&#x60; says &#x60;agent&#x60;), so the hint only tightens. The recorded identity of an agent is on the timeline item and the owner projection; see CONTRACTS.md.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { OpenAgentTabOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // OpenAgentTabRequest (optional)
    openAgentTabRequest: ...,
  } satisfies OpenAgentTabOperationRequest;

  try {
    const data = await api.openAgentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **openAgentTabRequest** | [OpenAgentTabRequest](OpenAgentTabRequest.md) |  | [Optional] |

### Return type

[**EnvelopeAgentTab**](EnvelopeAgentTab.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The new tab and the run\&#39;s tabs |  -  |
| **403** | Domain (including a &#x60;url&#x60; outside the run\&#39;s allowed domains), SSRF, grant or tier refusal |  -  |
| **404** | Run not found, not owned by the caller, or no longer live |  -  |
| **409** | USER_TOOK_OVER: the person using the browser has taken it over and this call is an agent\&#39;s. &#x60;error.details&#x60; is &#x60;{control_mode: \&quot;human_takeover\&quot;, control_epoch, taken_over_at}&#x60; (&#x60;taken_over_at&#x60;, RFC 3339, is absent for a run resumed into a takeover). Enforced only when QUARRY_EDGE__TAKEOVER_ENFORCEMENT is on. |  -  |
| **422** | The body does not parse: both spellings of the hint in one body, or a hint that is neither &#x60;human&#x60; nor &#x60;agent&#x60;. The answer is the HTTP framework\&#39;s plain-text rejection, not an error envelope. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## qualityCheckAgentProcedure

> EnvelopeProcedureQuality qualityCheckAgentProcedure(browserProcedure)

Evaluate deterministic procedure promotion evidence

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { QualityCheckAgentProcedureRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // BrowserProcedure
    browserProcedure: ...,
  } satisfies QualityCheckAgentProcedureRequest;

  try {
    const data = await api.qualityCheckAgentProcedure(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **browserProcedure** | [BrowserProcedure](BrowserProcedure.md) |  | |

### Return type

[**EnvelopeProcedureQuality**](EnvelopeProcedureQuality.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Quality report; does not change rollout state |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## releaseAgentAttach

> releaseAgentAttach(runId, attachId, disposition)

End an agent attachment (the delegate ends its own; the owner stops it)

Closes the agent tab and records the end on the timeline and as agent.detached. Does not revoke the grant.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ReleaseAgentAttachRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string
    attachId: attachId_example,
    // 'release' | 'abort' (optional)
    disposition: disposition_example,
  } satisfies ReleaseAgentAttachRequest;

  try {
    const data = await api.releaseAgentAttach(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **attachId** | `string` |  | [Defaults to `undefined`] |
| **disposition** | `release`, `abort` |  | [Optional] [Defaults to `undefined`] [Enum: release, abort] |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The attachment ended |  -  |
| **400** | Unknown disposition |  -  |
| **404** | Not the owner, not the live attachment\&#39;s delegate, or no such attachment |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## selectAgentTab

> EnvelopeAgentTabs selectAgentTab(runId, tabId, initiatedBy, aiRunId, agentLabel)

Make a tab the active one

Has no body, so the hint and the advisory fields are query parameters. Same take-over rule and recording as opening a tab.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { SelectAgentTabRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | A tab id from the run\'s tab list
    tabId: tabId_example,
    // 'human' | 'agent' | Who is making the call. Can only tighten: `agent` on a person\'s token makes it an agent\'s, and a service principal is always an agent. `initiated_by` is accepted too (both in one request is an error). (optional)
    initiatedBy: initiatedBy_example,
    // string | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. `ai_run_id` is accepted too. (optional)
    aiRunId: aiRunId_example,
    // string | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. `agent_label` is accepted too. (optional)
    agentLabel: agentLabel_example,
  } satisfies SelectAgentTabRequest;

  try {
    const data = await api.selectAgentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | A tab id from the run\&#39;s tab list | [Defaults to `undefined`] |
| **initiatedBy** | `human`, `agent` | Who is making the call. Can only tighten: &#x60;agent&#x60; on a person\&#39;s token makes it an agent\&#39;s, and a service principal is always an agent. &#x60;initiated_by&#x60; is accepted too (both in one request is an error). | [Optional] [Defaults to `undefined`] [Enum: human, agent] |
| **aiRunId** | `string` | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. &#x60;ai_run_id&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |
| **agentLabel** | `string` | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. &#x60;agent_label&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |

### Return type

[**EnvelopeAgentTabs**](EnvelopeAgentTabs.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The run\&#39;s tabs |  -  |
| **400** | A hint that is not &#x60;human&#x60; or &#x60;agent&#x60; |  -  |
| **404** | Run not found, not owned by the caller, or no longer live |  -  |
| **409** | USER_TOOK_OVER, as for opening a tab |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## setAgentTabHolder

> setAgentTabHolder(runId, tabId, setAgentTabHolderRequest)

Hand a tab to the agent or take it back (the owner\&#39;s own token only)

holder agent needs a live attachment on this tab (no_attached_agent, 409). holder human takes the tab back. Compare-and-set on the tab epoch: a stale expected_epoch is stale_tab_epoch (409). A change writes a tab_control timeline item.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { SetAgentTabHolderOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | A tab id from the run\'s tab list
    tabId: tabId_example,
    // SetAgentTabHolderRequest
    setAgentTabHolderRequest: ...,
  } satisfies SetAgentTabHolderOperationRequest;

  try {
    const data = await api.setAgentTabHolder(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | A tab id from the run\&#39;s tab list | [Defaults to `undefined`] |
| **setAgentTabHolderRequest** | [SetAgentTabHolderRequest](SetAgentTabHolderRequest.md) |  | |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The tab\&#39;s holder and epochs |  -  |
| **403** | A service principal or an agent token may not change the holder (control_transition_requires_human) |  -  |
| **404** | Run not found, or the tab is closed (pinned_tab_closed) |  -  |
| **409** | Stale epoch (stale_tab_epoch) or no live attachment (no_attached_agent) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## startAgentRun

> EnvelopeStartAgentRun startAgentRun(startAgentRunRequest)

Acquire a governed browser-agent run

Creates a tenant-owned browser lease. BrowserBroker grants are revalidated at run start and before every action; non-ZDR runs receive a durable continuation checkpoint. 

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { StartAgentRunOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // StartAgentRunRequest
    startAgentRunRequest: ...,
  } satisfies StartAgentRunOperationRequest;

  try {
    const data = await api.startAgentRun(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **startAgentRunRequest** | [StartAgentRunRequest](StartAgentRunRequest.md) |  | |

### Return type

[**EnvelopeStartAgentRun**](EnvelopeStartAgentRun.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Browser run acquired |  -  |
| **400** | Invalid constraints or persistence request |  -  |
| **403** | Missing or invalid BrowserBroker grant, or &#x60;policy_denied&#x60;: the request\&#39;s &#x60;policy&#x60; or the operator baseline refuses a capability the run asked for (&#x60;error.details.reason&#x60; &#x60;policy_denied&#x60;, with &#x60;capability&#x60;, &#x60;decision_source&#x60; and &#x60;retryable: false&#x60;). Nothing is acquired. A person\&#39;s &#x60;human_browse&#x60; grant (WS8.B; refused unless &#x60;QUARRY_EDGE__HUMAN_BROWSE&#x60; is on) is refused with 403 and the reason as the message: &#x60;human_browse_disabled&#x60;, &#x60;human_browse_service_refused&#x60;, &#x60;human_browse_owner_mismatch&#x60;, &#x60;human_browse_zdr_posture&#x60; or &#x60;human_browse_policy_unknown&#x60;. A resumed run whose stored policy mode differs is refused with \&quot;resumed run policy no longer matches its broker grant\&quot;. |  -  |
| **429** | RATE_LIMITED. A run cap is full: &#x60;error.details.reason&#x60; is &#x60;global_run_cap&#x60;, &#x60;org_run_cap&#x60;, &#x60;org_interactive_run_cap&#x60; or &#x60;org_plan_cap&#x60; (the organization\&#39;s plan bounds this class of run), with &#x60;scope&#x60;, &#x60;limit&#x60;, &#x60;active&#x60; and &#x60;retry_after_seconds&#x60;. Retryable. See ERROR_REASONS.md. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## streamAgentRunEvents

> string streamAgentRunEvents(runId, fromSeq, maxEvents, lastEventID)

Typed agent-run events as Server-Sent Events, with resume (WS10.S3)

Streams the run\&#39;s user-facing events (agent.started, action.started, action.completed, action.failed, observation.ready, agent.completed, agent.expired, agent.budget_exhausted, agent.failed) in the order of the run\&#39;s trace. Each SSE &#x60;id:&#x60; is the event\&#39;s per-run &#x60;seq&#x60; (dense, starting at 1 for an agent run). Resume with the &#x60;Last-Event-ID&#x60; header a reconnecting EventSource sends, or with &#x60;from_seq&#x60;; &#x60;from_seq&#x60; wins when both are present. The stream replays the run\&#39;s in-memory replay ring after that seq, then tails the live channel with no gap and no duplicate. The ring holds &#x60;QUARRY_EDGE__TRACE_RING_EVENTS&#x60; events (default 512; 0 disables it) within &#x60;QUARRY_EDGE__TRACE_RING_MAX_BYTES&#x60; (default 1 MiB) and lives only as long as the run. A &#x60;replay_gap&#x60; event comes first when the ring no longer holds the events after the resume point; its fallback is the durable browser-session timeline. The stream ends with &#x60;event: done&#x60; and &#x60;data: run_closed&#x60; when the run closes, or &#x60;data: max_events&#x60; when &#x60;max_events&#x60; is reached. A slow consumer gets &#x60;event: lagged&#x60; and the stream resumes from the ring. Only the owner of the run may read it; any other caller gets 404.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { StreamAgentRunEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // number | Replay the retained events with seq greater than this value before the live tail. Wins over Last-Event-ID. (optional)
    fromSeq: 56,
    // number | Close the stream after this many user-facing events (0 or absent, no cap). (optional)
    maxEvents: 56,
    // number | The seq of the last event the client received. Used when from_seq is absent. (optional)
    lastEventID: 56,
  } satisfies StreamAgentRunEventsRequest;

  try {
    const data = await api.streamAgentRunEvents(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **fromSeq** | `number` | Replay the retained events with seq greater than this value before the live tail. Wins over Last-Event-ID. | [Optional] [Defaults to `undefined`] |
| **maxEvents** | `number` | Close the stream after this many user-facing events (0 or absent, no cap). | [Optional] [Defaults to `undefined`] |
| **lastEventID** | `number` | The seq of the last event the client received. Used when from_seq is absent. | [Optional] [Defaults to `undefined`] |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/event-stream`, `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | text/event-stream. Named events: agent.started, action.started, action.completed, action.failed, observation.ready, agent.completed, agent.expired, agent.budget_exhausted, agent.failed (data is the Event envelope, id is seq), replay_gap (data is {requested, oldest_retained, fallback}), lagged (data is {skipped}), done (data is run_closed or max_events). |  -  |
| **404** | Run not found, or not owned by the caller |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

