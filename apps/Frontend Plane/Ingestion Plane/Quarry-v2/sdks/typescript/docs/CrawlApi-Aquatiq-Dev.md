# CrawlApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**crawlHandoff**](CrawlApi.md#crawlhandoff) | **POST** /v1/crawl | Start a crawl job |



## crawlHandoff

> EnvelopeHandoffAck crawlHandoff(crawlRequest)

Start a crawl job

Hands off a crawl request to the orchestrator for BFS execution. Returns a job ID for tracking progress. 

### Example

```ts
import {
  Configuration,
  CrawlApi,
} from '@quarry/client';
import type { CrawlHandoffRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new CrawlApi();

  const body = {
    // CrawlRequest
    crawlRequest: ...,
  } satisfies CrawlHandoffRequest;

  try {
    const data = await api.crawlHandoff(body);
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
| **crawlRequest** | [CrawlRequest](CrawlRequest.md) |  | |

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
| **200** | Crawl job accepted |  -  |
| **502** | Orchestrator unreachable |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

