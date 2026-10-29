# SearchApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**search**](SearchApi.md#searchoperation) | **POST** /v1/search | SERP + own-corpus search (hybrid when Data Plane wired) |



## search

> SearchResponse search(searchRequest)

SERP + own-corpus search (hybrid when Data Plane wired)

Provider-agnostic search across the local Tantivy corpus, SERP backends, and — when a Data Plane is configured — semantic vectors fused via RRF. Tavily-style params: topic, time_range/days, exact_match, chunks_per_source, include_answer, format&#x3D;context, highlight, facets. 

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { SearchOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // SearchRequest
    searchRequest: ...,
  } satisfies SearchOperationRequest;

  try {
    const data = await api.search(body);
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
| **searchRequest** | [SearchRequest](SearchRequest.md) |  | |

### Return type

[**SearchResponse**](SearchResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Ranked results (plus optional answer/context/facets) |  -  |
| **429** | Rate limited (structured retry envelope) |  -  |
| **501** | No search backend configured |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

