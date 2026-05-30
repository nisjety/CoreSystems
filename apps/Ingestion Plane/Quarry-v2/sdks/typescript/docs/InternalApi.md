# InternalApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**internalRunPage**](InternalApi.md#internalrunpage) | **POST** /v1/internal/run_page | Internal page execution (orchestrator → edge) |



## internalRunPage

> InternalRunPageResult internalRunPage(internalRunPage)

Internal page execution (orchestrator → edge)

Called by orchestrator activities to execute a single page. Not exposed publicly in production. 

### Example

```ts
import {
  Configuration,
  InternalApi,
} from '@quarry/client';
import type { InternalRunPageRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new InternalApi();

  const body = {
    // InternalRunPage
    internalRunPage: ...,
  } satisfies InternalRunPageRequest;

  try {
    const data = await api.internalRunPage(body);
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
| **internalRunPage** | [InternalRunPage](InternalRunPage.md) |  | |

### Return type

[**InternalRunPageResult**](InternalRunPageResult.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Page execution result |  -  |
| **400** | Invalid request |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

