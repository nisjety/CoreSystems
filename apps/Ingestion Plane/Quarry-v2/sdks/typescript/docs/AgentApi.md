# AgentApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**agentInteract**](AgentApi.md#agentinteract) | **POST** /v1/agent/runs/{run_id}/interact | Ergonomic alias for one governed browser action |
| [**agentStep**](AgentApi.md#agentstepoperation) | **POST** /v1/agent/runs/{run_id}/step | Execute one governed browser action |
| [**checkAgentProcedureReplay**](AgentApi.md#checkagentprocedurereplay) | **POST** /v1/agent/procedures/replay-check | Compare actions with a procedure without executing effects |
| [**closeAgentRun**](AgentApi.md#closeagentrun) | **DELETE** /v1/agent/runs/{run_id} | Close a browser-agent run and release its lease |
| [**closeAgentTab**](AgentApi.md#closeagenttab) | **DELETE** /v1/agent/runs/{run_id}/tabs/{tab_id} | Close a tab |
| [**compileAgentProcedure**](AgentApi.md#compileagentprocedure) | **POST** /v1/agent/runs/{run_id}/procedure | Compile verified receipts into a replay candidate |
| [**impactCheckAgentProcedure**](AgentApi.md#impactcheckagentprocedure) | **POST** /v1/agent/procedures/impact-check | Identify changed sources that require procedure quarantine |
| [**listAgentEgressReceipts**](AgentApi.md#listagentegressreceipts) | **GET** /v1/agent/runs/{run_id}/egress-receipts | Read redacted live browser egress decisions |
| [**listAgentReceipts**](AgentApi.md#listagentreceipts) | **GET** /v1/agent/runs/{run_id}/receipts | Read tenant-scoped immutable action receipts |
| [**listAgentTabs**](AgentApi.md#listagenttabs) | **GET** /v1/agent/runs/{run_id}/tabs | List the live browser tabs of a run |
| [**openAgentTab**](AgentApi.md#openagenttaboperation) | **POST** /v1/agent/runs/{run_id}/tabs | Open a browser tab on a run |
| [**qualityCheckAgentProcedure**](AgentApi.md#qualitycheckagentprocedure) | **POST** /v1/agent/procedures/quality-check | Evaluate deterministic procedure promotion evidence |
| [**selectAgentTab**](AgentApi.md#selectagenttab) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/select | Make a tab the active one |
| [**startAgentRun**](AgentApi.md#startagentrunoperation) | **POST** /v1/agent/runs | Acquire a governed browser-agent run |



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
| **409** | Either the snapshot target is stale, absent, ambiguous, or changed (re-observe before acting), or USER_TOOK_OVER: the person using the browser has taken it over and this action was proposed by an agent (a service principal, or &#x60;initiated_by: agent&#x60;). &#x60;error.details&#x60; is &#x60;{control_mode: \&quot;human_takeover\&quot;, control_epoch}&#x60;. Not retryable until control is handed back. Enforced only when QUARRY_EDGE__TAKEOVER_ENFORCEMENT&#x3D;1; otherwise the edge logs what it would have refused. |  -  |
| **402** | BUDGET_EXHAUSTED. An agent run reached its step, attempt, runtime or estimated-cost bound (error.details.dimension is steps, attempts, runtime or cost, with limit, spent and unit). Not retryable: the budget does not refill, and a resumed run keeps what it spent. Only returned when QUARRY_EDGE__BUDGET_ENFORCEMENT&#x3D;on; in shadow mode the same condition is only recorded. |  -  |
| **429** | Step budget exhausted (the step limit the request itself set) |  -  |

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
| **409** | USER_TOOK_OVER: the person using the browser has taken it over and this call is an agent\&#39;s. &#x60;error.details&#x60; is &#x60;{control_mode: \&quot;human_takeover\&quot;, control_epoch}&#x60;. Enforced only when QUARRY_EDGE__TAKEOVER_ENFORCEMENT is on. |  -  |
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
| **403** | Missing or invalid BrowserBroker grant |  -  |
| **429** | RATE_LIMITED. A run cap is full: &#x60;error.details.reason&#x60; is &#x60;global_run_cap&#x60;, &#x60;org_run_cap&#x60; or &#x60;org_interactive_run_cap&#x60;, with &#x60;scope&#x60;, &#x60;limit&#x60;, &#x60;active&#x60; and &#x60;retry_after_seconds&#x60;. Retryable. See ERROR_REASONS.md. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

