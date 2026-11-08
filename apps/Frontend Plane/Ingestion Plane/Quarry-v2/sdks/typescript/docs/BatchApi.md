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
| **400** | WS14.S5. Every URL of the batch was refused by the static check, so no URL can be crawled. Body is BAD_REQUEST with details.rejected (url, code, message). Nothing is created. A batch with at least one crawlable URL is a 200 with HandoffAck.rejected. |  -  |
| **402** | The organization\&#39;s configured batch URL (quarry.batch.url) allowance is spent (WS6.D-s; only a source that reports an allowance produces this, the default static source never does). Body is BUDGET_EXHAUSTED with details dimension org_allowance, metric, limit_source entitlement and resumable false. No job is created. |  -  |
| **429** | Backlog full (QUARRY_JOB_ADMISSION on): the organization already has its bound of waiting jobs. Body is RATE_LIMITED with details reason org_backlog_full, limit, queued and retry_after_seconds, and a Retry-After header. Nothing is stored. |  -  |
| **502** | Orchestrator unreachable |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

