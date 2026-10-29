# BatchApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**batchHandoff**](BatchApi.md#batchhandoff) | **POST** /v1/batch | Start a batch scrape job |



## batchHandoff

> EnvelopeHandoffAck batchHandoff(batchRequest)

Start a batch scrape job

Hands off multiple URLs to the orchestrator for parallel execution. Returns a job ID for tracking progress. 

### Example

```ts
import {
  Configuration,
  BatchApi,
} from '@quarry/client';
import type { BatchHandoffRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new BatchApi();

  const body = {
    // BatchRequest
    batchRequest: ...,
  } satisfies BatchHandoffRequest;

  try {
    const data = await api.batchHandoff(body);
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
| **batchRequest** | [BatchRequest](BatchRequest.md) |  | |

### Return type

[**EnvelopeHandoffAck**](EnvelopeHandoffAck.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Batch job accepted |  -  |
| **502** | Orchestrator unreachable |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

