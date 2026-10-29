# TasksApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**cancelTask**](TasksApi.md#canceltask) | **POST** /v1/tasks/{id}/cancel | Cancel a task |
| [**createTask**](TasksApi.md#createtask) | **POST** /v1/tasks | Create a task |
| [**getTask**](TasksApi.md#gettask) | **GET** /v1/tasks/{id} | Get a task by ID |
| [**listTasks**](TasksApi.md#listtasks) | **GET** /v1/tasks | List tasks |
| [**patchTask**](TasksApi.md#patchtask) | **PATCH** /v1/tasks/{id} | Update a task |



## cancelTask

> { [key: string]: any; } cancelTask(id)

Cancel a task

### Example

```ts
import {
  Configuration,
  TasksApi,
} from '@model-plane/sdk';
import type { CancelTaskRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new TasksApi(config);

  const body = {
    // string
    id: id_example,
  } satisfies CancelTaskRequest;

  try {
    const data = await api.cancelTask(body);
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
| **id** | `string` |  | [Defaults to `undefined`] |

### Return type

**{ [key: string]: any; }**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Task cancelled |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## createTask

> { [key: string]: any; } createTask(taskInput)

Create a task

### Example

```ts
import {
  Configuration,
  TasksApi,
} from '@model-plane/sdk';
import type { CreateTaskRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new TasksApi(config);

  const body = {
    // TaskInput
    taskInput: ...,
  } satisfies CreateTaskRequest;

  try {
    const data = await api.createTask(body);
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
| **taskInput** | [TaskInput](TaskInput.md) |  | |

### Return type

**{ [key: string]: any; }**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Task created |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getTask

> { [key: string]: any; } getTask(id)

Get a task by ID

### Example

```ts
import {
  Configuration,
  TasksApi,
} from '@model-plane/sdk';
import type { GetTaskRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new TasksApi(config);

  const body = {
    // string
    id: id_example,
  } satisfies GetTaskRequest;

  try {
    const data = await api.getTask(body);
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
| **id** | `string` |  | [Defaults to `undefined`] |

### Return type

**{ [key: string]: any; }**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Task details |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listTasks

> { [key: string]: any; } listTasks(status)

List tasks

### Example

```ts
import {
  Configuration,
  TasksApi,
} from '@model-plane/sdk';
import type { ListTasksRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new TasksApi(config);

  const body = {
    // string (optional)
    status: status_example,
  } satisfies ListTasksRequest;

  try {
    const data = await api.listTasks(body);
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
| **status** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

**{ [key: string]: any; }**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Task list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## patchTask

> { [key: string]: any; } patchTask(id, requestBody)

Update a task

### Example

```ts
import {
  Configuration,
  TasksApi,
} from '@model-plane/sdk';
import type { PatchTaskRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new TasksApi(config);

  const body = {
    // string
    id: id_example,
    // { [key: string]: any; }
    requestBody: Object,
  } satisfies PatchTaskRequest;

  try {
    const data = await api.patchTask(body);
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
| **id** | `string` |  | [Defaults to `undefined`] |
| **requestBody** | `{ [key: string]: any; }` |  | |

### Return type

**{ [key: string]: any; }**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Task updated |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

