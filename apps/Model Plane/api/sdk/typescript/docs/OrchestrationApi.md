# OrchestrationApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**approvePlan**](OrchestrationApi.md#approveplan) | **POST** /v1/orchestration/plans/{plan_id}/approve | Approve a plan |
| [**cancelRun**](OrchestrationApi.md#cancelrun) | **POST** /v1/orchestration/runs/{run_id}/cancel | Request cancellation of a run |
| [**decideApproval**](OrchestrationApi.md#decideapproval) | **POST** /v1/orchestration/approvals/{approval_id}/decide | Approve or reject an approval request |
| [**getApproval**](OrchestrationApi.md#getapproval) | **GET** /v1/orchestration/approvals/{approval_id} | Get an approval by ID |
| [**getPlan**](OrchestrationApi.md#getplan) | **GET** /v1/orchestration/plans/{plan_id} | Get a plan by ID |
| [**getSubagentLineage**](OrchestrationApi.md#getsubagentlineage) | **GET** /v1/orchestration/threads/{thread_id}/lineage | Get subagent lineage for a thread |
| [**getTodo**](OrchestrationApi.md#gettodo) | **GET** /v1/orchestration/todos/{todo_id} | Get a todo by ID |
| [**listApprovals**](OrchestrationApi.md#listapprovals) | **GET** /v1/orchestration/runs/{run_id}/approvals | List approvals for a run |
| [**listPlans**](OrchestrationApi.md#listplans) | **GET** /v1/orchestration/runs/{run_id}/plans | List plans for a run |
| [**listTodos**](OrchestrationApi.md#listtodos) | **GET** /v1/orchestration/threads/{thread_id}/todos | List todos for a thread |
| [**rejectPlan**](OrchestrationApi.md#rejectplan) | **POST** /v1/orchestration/plans/{plan_id}/reject | Reject a plan |
| [**resumeRun**](OrchestrationApi.md#resumerun) | **POST** /v1/orchestration/runs/{run_id}/resume | Request resumption of a cancelled/paused run |
| [**updateTodoStatus**](OrchestrationApi.md#updatetodostatus) | **POST** /v1/orchestration/todos/{todo_id}/status | Transition a todo to a new status |



## approvePlan

> GetPlan200Response approvePlan(planId, transitionPlanBody)

Approve a plan

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { ApprovePlanRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    planId: planId_example,
    // TransitionPlanBody
    transitionPlanBody: ...,
  } satisfies ApprovePlanRequest;

  try {
    const data = await api.approvePlan(body);
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
| **planId** | `string` |  | [Defaults to `undefined`] |
| **transitionPlanBody** | [TransitionPlanBody](TransitionPlanBody.md) |  | |

### Return type

[**GetPlan200Response**](GetPlan200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Plan approved |  -  |
| **400** | Invalid request |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## cancelRun

> CancelRun200Response cancelRun(runId)

Request cancellation of a run

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { CancelRunRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    runId: runId_example,
  } satisfies CancelRunRequest;

  try {
    const data = await api.cancelRun(body);
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
| **runId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**CancelRun200Response**](CancelRun200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Cancellation requested |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## decideApproval

> GetApproval200Response decideApproval(approvalId, decideApprovalBody)

Approve or reject an approval request

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { DecideApprovalRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    approvalId: approvalId_example,
    // DecideApprovalBody
    decideApprovalBody: ...,
  } satisfies DecideApprovalRequest;

  try {
    const data = await api.decideApproval(body);
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
| **approvalId** | `string` |  | [Defaults to `undefined`] |
| **decideApprovalBody** | [DecideApprovalBody](DecideApprovalBody.md) |  | |

### Return type

[**GetApproval200Response**](GetApproval200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Approval decided |  -  |
| **400** | Invalid request |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getApproval

> GetApproval200Response getApproval(approvalId)

Get an approval by ID

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { GetApprovalRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    approvalId: approvalId_example,
  } satisfies GetApprovalRequest;

  try {
    const data = await api.getApproval(body);
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
| **approvalId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**GetApproval200Response**](GetApproval200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Approval details |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getPlan

> GetPlan200Response getPlan(planId)

Get a plan by ID

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { GetPlanRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    planId: planId_example,
  } satisfies GetPlanRequest;

  try {
    const data = await api.getPlan(body);
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
| **planId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**GetPlan200Response**](GetPlan200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Plan details |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getSubagentLineage

> GetSubagentLineage200Response getSubagentLineage(threadId)

Get subagent lineage for a thread

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { GetSubagentLineageRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    threadId: threadId_example,
  } satisfies GetSubagentLineageRequest;

  try {
    const data = await api.getSubagentLineage(body);
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
| **threadId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**GetSubagentLineage200Response**](GetSubagentLineage200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Lineage graph |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getTodo

> GetTodo200Response getTodo(todoId)

Get a todo by ID

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { GetTodoRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    todoId: todoId_example,
  } satisfies GetTodoRequest;

  try {
    const data = await api.getTodo(body);
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
| **todoId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**GetTodo200Response**](GetTodo200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Todo details |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listApprovals

> ListApprovals200Response listApprovals(runId, stepId)

List approvals for a run

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { ListApprovalsRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    runId: runId_example,
    // string (optional)
    stepId: stepId_example,
  } satisfies ListApprovalsRequest;

  try {
    const data = await api.listApprovals(body);
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
| **runId** | `string` |  | [Defaults to `undefined`] |
| **stepId** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

[**ListApprovals200Response**](ListApprovals200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Approvals list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listPlans

> ListPlans200Response listPlans(runId)

List plans for a run

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { ListPlansRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    runId: runId_example,
  } satisfies ListPlansRequest;

  try {
    const data = await api.listPlans(body);
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
| **runId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**ListPlans200Response**](ListPlans200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Plans list |  -  |
| **401** | Missing or invalid bearer token |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listTodos

> ListTodos200Response listTodos(threadId, runId)

List todos for a thread

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { ListTodosRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    threadId: threadId_example,
    // string (optional)
    runId: runId_example,
  } satisfies ListTodosRequest;

  try {
    const data = await api.listTodos(body);
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
| **threadId** | `string` |  | [Defaults to `undefined`] |
| **runId** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

[**ListTodos200Response**](ListTodos200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Todos list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## rejectPlan

> GetPlan200Response rejectPlan(planId, transitionPlanBody)

Reject a plan

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { RejectPlanRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    planId: planId_example,
    // TransitionPlanBody
    transitionPlanBody: ...,
  } satisfies RejectPlanRequest;

  try {
    const data = await api.rejectPlan(body);
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
| **planId** | `string` |  | [Defaults to `undefined`] |
| **transitionPlanBody** | [TransitionPlanBody](TransitionPlanBody.md) |  | |

### Return type

[**GetPlan200Response**](GetPlan200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Plan rejected |  -  |
| **400** | Invalid request |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## resumeRun

> ResumeRun200Response resumeRun(runId)

Request resumption of a cancelled/paused run

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { ResumeRunRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    runId: runId_example,
  } satisfies ResumeRunRequest;

  try {
    const data = await api.resumeRun(body);
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
| **runId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**ResumeRun200Response**](ResumeRun200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Resumption requested |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## updateTodoStatus

> GetTodo200Response updateTodoStatus(todoId, transitionTodoBody)

Transition a todo to a new status

### Example

```ts
import {
  Configuration,
  OrchestrationApi,
} from '@model-plane/sdk';
import type { UpdateTodoStatusRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new OrchestrationApi(config);

  const body = {
    // string
    todoId: todoId_example,
    // TransitionTodoBody
    transitionTodoBody: ...,
  } satisfies UpdateTodoStatusRequest;

  try {
    const data = await api.updateTodoStatus(body);
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
| **todoId** | `string` |  | [Defaults to `undefined`] |
| **transitionTodoBody** | [TransitionTodoBody](TransitionTodoBody.md) |  | |

### Return type

[**GetTodo200Response**](GetTodo200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Todo updated |  -  |
| **400** | Invalid request |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

