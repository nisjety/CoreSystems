# ExtractApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**extract**](ExtractApi.md#extractoperation) | **POST** /v1/extract | Multi-URL structured extraction |



## extract

> ExtractResponse extract(extractRequest)

Multi-URL structured extraction

Fetch a bounded set of URLs and return structured data per source (JSON-Schema-guided via Model Plane) or cleaned markdown. Per-source isolation — one failure never sinks the batch. 

### Example

```ts
import {
  Configuration,
  ExtractApi,
} from '@quarry/client';
import type { ExtractOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ExtractApi();

  const body = {
    // ExtractRequest
    extractRequest: ...,
  } satisfies ExtractOperationRequest;

  try {
    const data = await api.extract(body);
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
| **extractRequest** | [ExtractRequest](ExtractRequest.md) |  | |

### Return type

[**ExtractResponse**](ExtractResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Per-source extraction results |  -  |
| **400** | No valid URLs in request |  -  |
| **501** | Structured extraction requires a Model Plane |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

