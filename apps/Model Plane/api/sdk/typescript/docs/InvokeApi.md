# InvokeApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**invoke**](InvokeApi.md#invokeoperation) | **POST** /v1/invoke | Synchronous inference request |
| [**invokeStream**](InvokeApi.md#invokestream) | **POST** /v1/invoke/stream | Streaming inference via SSE |



## invoke

> InvokeResponse invoke(invokeRequest)

Synchronous inference request

Sends content to the configured model via inference-core and returns the full response. Budget is checked pre-flight against cost-core. A session run is created in session-core for context tracking. 

### Example

```ts
import {
  Configuration,
  InvokeApi,
} from '@model-plane/sdk';
import type { InvokeOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new InvokeApi(config);

  const body = {
    // InvokeRequest
    invokeRequest: ...,
  } satisfies InvokeOperationRequest;

  try {
    const data = await api.invoke(body);
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
| **invokeRequest** | [InvokeRequest](InvokeRequest.md) |  | |

### Return type

[**InvokeResponse**](InvokeResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Inference completed |  -  |
| **400** | Invalid request |  -  |
| **401** | Missing or invalid bearer token |  -  |
| **402** | Budget exceeded |  -  |
| **502** | Upstream service error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## invokeStream

> SseChunk invokeStream(invokeRequest)

Streaming inference via SSE

Same as /v1/invoke but returns Server-Sent Events. Each event carries a delta chunk. The final event has &#x60;done: true&#x60;. Event type &#x60;done&#x60; signals end of stream. 

### Example

```ts
import {
  Configuration,
  InvokeApi,
} from '@model-plane/sdk';
import type { InvokeStreamRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new InvokeApi(config);

  const body = {
    // InvokeRequest
    invokeRequest: ...,
  } satisfies InvokeStreamRequest;

  try {
    const data = await api.invokeStream(body);
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
| **invokeRequest** | [InvokeRequest](InvokeRequest.md) |  | |

### Return type

[**SseChunk**](SseChunk.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `text/event-stream`, `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | SSE stream of inference chunks |  -  |
| **400** | Invalid request |  -  |
| **401** | Missing or invalid bearer token |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

