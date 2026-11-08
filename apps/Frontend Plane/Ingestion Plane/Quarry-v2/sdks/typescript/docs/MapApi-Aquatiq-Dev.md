# MapApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**map**](MapApi.md#mapoperation) | **POST** /v1/map | Whole-site URL discovery |



## map

> MapResponse map(mapRequest)

Whole-site URL discovery

Fast site map from sitemap.xml + robots + on-page links, optionally relevance-ranked by &#x60;search&#x60;. Read-only, org-scoped, SSRF-guarded. 

### Example

```ts
import {
  Configuration,
  MapApi,
} from '@quarry/client';
import type { MapOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new MapApi();

  const body = {
    // MapRequest
    mapRequest: ...,
  } satisfies MapOperationRequest;

  try {
    const data = await api.map(body);
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
| **mapRequest** | [MapRequest](MapRequest.md) |  | |

### Return type

[**MapResponse**](MapResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Discovered URLs |  -  |
| **502** | Origin returned no sitemap or links |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

