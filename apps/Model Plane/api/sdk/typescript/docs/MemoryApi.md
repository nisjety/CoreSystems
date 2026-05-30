# MemoryApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**createMemory**](MemoryApi.md#creatememory) | **POST** /v1/memory | Create a memory entry |
| [**deleteMemory**](MemoryApi.md#deletememory) | **DELETE** /v1/memory/{id} | Delete a memory entry |
| [**getMemory**](MemoryApi.md#getmemory) | **GET** /v1/memory/{id} | Get a memory entry by ID |
| [**listMemory**](MemoryApi.md#listmemory) | **GET** /v1/memory | List memory entries |
| [**patchMemory**](MemoryApi.md#patchmemory) | **PATCH** /v1/memory/{id} | Update a memory entry |



## createMemory

> { [key: string]: any; } createMemory(requestBody)

Create a memory entry

### Example

```ts
import {
  Configuration,
  MemoryApi,
} from '@model-plane/sdk';
import type { CreateMemoryRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new MemoryApi(config);

  const body = {
    // { [key: string]: any; }
    requestBody: Object,
  } satisfies CreateMemoryRequest;

  try {
    const data = await api.createMemory(body);
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
| **200** | Memory created |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## deleteMemory

> { [key: string]: any; } deleteMemory(id)

Delete a memory entry

### Example

```ts
import {
  Configuration,
  MemoryApi,
} from '@model-plane/sdk';
import type { DeleteMemoryRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new MemoryApi(config);

  const body = {
    // string
    id: id_example,
  } satisfies DeleteMemoryRequest;

  try {
    const data = await api.deleteMemory(body);
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
| **200** | Memory deleted |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getMemory

> { [key: string]: any; } getMemory(id)

Get a memory entry by ID

### Example

```ts
import {
  Configuration,
  MemoryApi,
} from '@model-plane/sdk';
import type { GetMemoryRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new MemoryApi(config);

  const body = {
    // string
    id: id_example,
  } satisfies GetMemoryRequest;

  try {
    const data = await api.getMemory(body);
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
| **200** | Memory details |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listMemory

> { [key: string]: any; } listMemory(scope)

List memory entries

### Example

```ts
import {
  Configuration,
  MemoryApi,
} from '@model-plane/sdk';
import type { ListMemoryRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new MemoryApi(config);

  const body = {
    // string (optional)
    scope: scope_example,
  } satisfies ListMemoryRequest;

  try {
    const data = await api.listMemory(body);
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
| **scope** | `string` |  | [Optional] [Defaults to `undefined`] |

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
| **200** | Memory list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## patchMemory

> { [key: string]: any; } patchMemory(id, requestBody)

Update a memory entry

### Example

```ts
import {
  Configuration,
  MemoryApi,
} from '@model-plane/sdk';
import type { PatchMemoryRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new MemoryApi(config);

  const body = {
    // string
    id: id_example,
    // { [key: string]: any; }
    requestBody: Object,
  } satisfies PatchMemoryRequest;

  try {
    const data = await api.patchMemory(body);
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
| **200** | Memory updated |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

