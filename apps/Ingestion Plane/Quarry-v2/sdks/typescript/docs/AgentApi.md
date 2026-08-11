# AgentApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**agentInteract**](AgentApi.md#agentinteract) | **POST** /v1/agent/runs/{run_id}/interact | Ergonomic alias for one governed browser action |
| [**agentStep**](AgentApi.md#agentstepoperation) | **POST** /v1/agent/runs/{run_id}/step | Execute one governed browser action |
| [**checkAgentProcedureReplay**](AgentApi.md#checkagentprocedurereplay) | **POST** /v1/agent/procedures/replay-check | Compare actions with a procedure without executing effects |
| [**closeAgentRun**](AgentApi.md#closeagentrun) | **DELETE** /v1/agent/runs/{run_id} | Close a browser-agent run and release its lease |
| [**compileAgentProcedure**](AgentApi.md#compileagentprocedure) | **POST** /v1/agent/runs/{run_id}/procedure | Compile verified receipts into a replay candidate |
| [**impactCheckAgentProcedure**](AgentApi.md#impactcheckagentprocedure) | **POST** /v1/agent/procedures/impact-check | Identify changed sources that require procedure quarantine |
| [**listAgentReceipts**](AgentApi.md#listagentreceipts) | **GET** /v1/agent/runs/{run_id}/receipts | Read tenant-scoped immutable action receipts |
| [**qualityCheckAgentProcedure**](AgentApi.md#qualitycheckagentprocedure) | **POST** /v1/agent/procedures/quality-check | Evaluate deterministic procedure promotion evidence |
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
| **429** | Step budget exhausted |  -  |

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

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

