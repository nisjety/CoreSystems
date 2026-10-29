# AiApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**aiChat**](AiApi.md#aichatoperation) | **POST** /v1/ai/chat | Chat completion via inference-core |
| [**aiDocuments**](AiApi.md#aidocumentsoperation) | **POST** /v1/ai/documents | Document processing (queued) |
| [**aiImages**](AiApi.md#aiimagesoperation) | **POST** /v1/ai/images | Image generation (queued) |
| [**aiRealtime**](AiApi.md#airealtimeoperation) | **POST** /v1/ai/realtime | Realtime session placeholder |
| [**aiSpeech**](AiApi.md#aispeechoperation) | **POST** /v1/ai/speech | Text-to-speech synthesis (queued) |
| [**aiTranslate**](AiApi.md#aitranslateoperation) | **POST** /v1/ai/translate | Text translation (queued) |



## aiChat

> AiChatResponse aiChat(aiChatRequest)

Chat completion via inference-core

### Example

```ts
import {
  Configuration,
  AiApi,
} from '@model-plane/sdk';
import type { AiChatOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new AiApi(config);

  const body = {
    // AiChatRequest
    aiChatRequest: ...,
  } satisfies AiChatOperationRequest;

  try {
    const data = await api.aiChat(body);
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
| **aiChatRequest** | [AiChatRequest](AiChatRequest.md) |  | |

### Return type

[**AiChatResponse**](AiChatResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Chat response |  -  |
| **400** | Invalid request |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## aiDocuments

> QueuedResponse aiDocuments(aiDocumentsRequest)

Document processing (queued)

### Example

```ts
import {
  Configuration,
  AiApi,
} from '@model-plane/sdk';
import type { AiDocumentsOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new AiApi(config);

  const body = {
    // AiDocumentsRequest
    aiDocumentsRequest: ...,
  } satisfies AiDocumentsOperationRequest;

  try {
    const data = await api.aiDocuments(body);
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
| **aiDocumentsRequest** | [AiDocumentsRequest](AiDocumentsRequest.md) |  | |

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Processing queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## aiImages

> QueuedResponse aiImages(aiImagesRequest)

Image generation (queued)

### Example

```ts
import {
  Configuration,
  AiApi,
} from '@model-plane/sdk';
import type { AiImagesOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new AiApi(config);

  const body = {
    // AiImagesRequest
    aiImagesRequest: ...,
  } satisfies AiImagesOperationRequest;

  try {
    const data = await api.aiImages(body);
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
| **aiImagesRequest** | [AiImagesRequest](AiImagesRequest.md) |  | |

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Generation queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## aiRealtime

> QueuedResponse aiRealtime(aiRealtimeRequest)

Realtime session placeholder

Realtime sessions require WebSocket upgrade. Use /v1/invoke/stream for SSE streaming instead. 

### Example

```ts
import {
  Configuration,
  AiApi,
} from '@model-plane/sdk';
import type { AiRealtimeOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new AiApi(config);

  const body = {
    // AiRealtimeRequest
    aiRealtimeRequest: ...,
  } satisfies AiRealtimeOperationRequest;

  try {
    const data = await api.aiRealtime(body);
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
| **aiRealtimeRequest** | [AiRealtimeRequest](AiRealtimeRequest.md) |  | |

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Not implemented — use SSE streaming |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## aiSpeech

> QueuedResponse aiSpeech(aiSpeechRequest)

Text-to-speech synthesis (queued)

### Example

```ts
import {
  Configuration,
  AiApi,
} from '@model-plane/sdk';
import type { AiSpeechOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new AiApi(config);

  const body = {
    // AiSpeechRequest
    aiSpeechRequest: ...,
  } satisfies AiSpeechOperationRequest;

  try {
    const data = await api.aiSpeech(body);
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
| **aiSpeechRequest** | [AiSpeechRequest](AiSpeechRequest.md) |  | |

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Synthesis queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## aiTranslate

> QueuedResponse aiTranslate(aiTranslateRequest)

Text translation (queued)

### Example

```ts
import {
  Configuration,
  AiApi,
} from '@model-plane/sdk';
import type { AiTranslateOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new AiApi(config);

  const body = {
    // AiTranslateRequest
    aiTranslateRequest: ...,
  } satisfies AiTranslateOperationRequest;

  try {
    const data = await api.aiTranslate(body);
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
| **aiTranslateRequest** | [AiTranslateRequest](AiTranslateRequest.md) |  | |

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Translation queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

