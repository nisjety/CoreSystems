# OpsApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**health**](OpsApi.md#health) | **GET** /health | Liveness probe |
| [**ready**](OpsApi.md#ready) | **GET** /ready | Readiness probe |



## health

> string health()

Liveness probe

### Example

```ts
import {
  Configuration,
  OpsApi,
} from '@quarry/client';
import type { HealthRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new OpsApi();

  try {
    const data = await api.health();
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters

This endpoint does not need any parameter.

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/plain`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Service is alive |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## ready

> Ready200Response ready()

Readiness probe

Whether this edge should receive traffic. 503 while it drains for shutdown, while its stores are not durable, and when a local browser is unhealthy. The edge runs two local Chromium drivers: agent runs\&#39; (&#x60;browser&#x60;) and the scrape path\&#39;s (&#x60;scrape_browser&#x60;); each object is present only when that driver runs a local browser, and a driver without one (a remote provider) leaves the body as &#x60;{\&quot;durable\&quot;:true,\&quot;status\&quot;:\&quot;ready\&quot;}&#x60;. A browser that was never launched is &#x60;idle&#x60; and ready: the first run launches it. &#x60;/health&#x60; never changes with any of this.

### Example

```ts
import {
  Configuration,
  OpsApi,
} from '@quarry/client';
import type { ReadyRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new OpsApi();

  try {
    const data = await api.ready();
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**Ready200Response**](Ready200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Service is ready to accept traffic |  -  |
| **503** | Not ready. &#x60;status: draining&#x60; (the body has no other key) while the edge shuts down; &#x60;status: not_ready&#x60; with &#x60;durable: false&#x60; and the store\&#39;s &#x60;reason&#x60; when the stores are not durable; &#x60;status: not_ready&#x60;, &#x60;durable: true&#x60;, &#x60;reason: browser_unhealthy&#x60; when the agent or the scrape browser is &#x60;unhealthy&#x60; (every launched browser of that driver failed two health probes in a row, or its process is dead), with both browser objects as in the 200 body. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

