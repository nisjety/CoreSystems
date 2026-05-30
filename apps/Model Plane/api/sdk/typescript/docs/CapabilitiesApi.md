# CapabilitiesApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**getCapability**](CapabilitiesApi.md#getcapability) | **GET** /v1/capabilities/{id} | Get a capability by ID |
| [**listCapabilities**](CapabilitiesApi.md#listcapabilities) | **GET** /v1/capabilities | List registered capabilities |



## getCapability

> Capability getCapability(id)

Get a capability by ID

### Example

```ts
import {
  Configuration,
  CapabilitiesApi,
} from '@model-plane/sdk';
import type { GetCapabilityRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new CapabilitiesApi(config);

  const body = {
    // string
    id: id_example,
  } satisfies GetCapabilityRequest;

  try {
    const data = await api.getCapability(body);
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
| **id** | `string` |  | [Defaults to `undefined`] |

### Return type

[**Capability**](Capability.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Capability details |  -  |
| **404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listCapabilities

> ListCapabilities200Response listCapabilities(kind, q, afterId, limit)

List registered capabilities

### Example

```ts
import {
  Configuration,
  CapabilitiesApi,
} from '@model-plane/sdk';
import type { ListCapabilitiesRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new CapabilitiesApi(config);

  const body = {
    // string (optional)
    kind: kind_example,
    // string | Full-text search query (optional)
    q: q_example,
    // string | Cursor for pagination (optional)
    afterId: afterId_example,
    // number (optional)
    limit: 56,
  } satisfies ListCapabilitiesRequest;

  try {
    const data = await api.listCapabilities(body);
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
| **kind** | `string` |  | [Optional] [Defaults to `undefined`] |
| **q** | `string` | Full-text search query | [Optional] [Defaults to `undefined`] |
| **afterId** | `string` | Cursor for pagination | [Optional] [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `50`] |

### Return type

[**ListCapabilities200Response**](ListCapabilities200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Capabilities list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

