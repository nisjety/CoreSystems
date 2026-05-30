# HealthApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**healthz**](HealthApi.md#healthz) | **GET** /healthz | Liveness probe |
| [**metrics**](HealthApi.md#metrics) | **GET** /metrics | Prometheus metrics |
| [**readyz**](HealthApi.md#readyz) | **GET** /readyz | Readiness probe |



## healthz

> string healthz()

Liveness probe

### Example

```ts
import {
  Configuration,
  HealthApi,
} from '@model-plane/sdk';
import type { HealthzRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const api = new HealthApi();

  try {
    const data = await api.healthz();
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
| **200** | Healthy |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## metrics

> string metrics()

Prometheus metrics

### Example

```ts
import {
  Configuration,
  HealthApi,
} from '@model-plane/sdk';
import type { MetricsRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const api = new HealthApi();

  try {
    const data = await api.metrics();
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
| **200** | Prometheus text exposition |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## readyz

> string readyz()

Readiness probe

### Example

```ts
import {
  Configuration,
  HealthApi,
} from '@model-plane/sdk';
import type { ReadyzRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const api = new HealthApi();

  try {
    const data = await api.readyz();
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
| **200** | Ready |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

