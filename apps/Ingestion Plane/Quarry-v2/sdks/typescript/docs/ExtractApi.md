# ExtractApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**extract**](ExtractApi.md#extractoperation) | **POST** /v1/extract | Multi-URL structured extraction |



## extract

> ExtractResponse extract(extractRequest)

Multi-URL structured extraction

Fetch a bounded set of URLs and return structured data per source (JSON-Schema-guided via Model Plane) or cleaned markdown. Per-source isolation — one failure never sinks the batch.  &#x60;schema&#x60; alone is schema-guided extraction. &#x60;prompt&#x60; alone asks the model for one free-form JSON object (&#x60;data&#x60;, status &#x60;ok&#x60;, no &#x60;schema_valid&#x60;). Both together is schema-guided extraction with the prompt as extra instructions. Neither returns cleaned markdown (status &#x60;fetched&#x60;). Under zero data retention no page text is sent to a model: each source answers status &#x60;error&#x60; with a &#x60;ZDR_UNAVAILABLE&#x60; message, and its &#x60;structured&#x60; harvest still comes back.  WS16.S6: the model arm runs on the AI scrape engine (&#x60;POST /v1/ai/chat&#x60; on the Model Plane), one &#x60;json&#x60; run per source, sources concurrently (at most &#x60;QUARRY_EDGE__AI_FORMATS_CONCURRENCY&#x60;), results in the order of &#x60;urls&#x60;. Each source of a model request carries &#x60;ai&#x60;, the json &#x60;AiFormatReport&#x60;. Off unless &#x60;QUARRY_EDGE__AI_FORMATS&#x60; is on. 

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
| **400** | No valid URLs in request, a &#x60;prompt&#x60; that is empty after trimming or over 10000 characters, or a &#x60;schema&#x60; the AI scrape engine cannot use (not an object, too large, or a root without &#x60;\&quot;type\&quot;: \&quot;object\&quot;&#x60;) |  -  |
| **403** | The web policy refused a target, or (WS16.S6) a service principal without the &#x60;extract:ai&#x60; scope sent a &#x60;schema&#x60; or a &#x60;prompt&#x60;. Decided before any fetch. |  -  |
| **501** | A &#x60;schema&#x60; or a &#x60;prompt&#x60; was sent and the Model Plane cannot be used (never for a ZDR request, and never when neither field is sent). The body is &#x60;{error, code, hint, next_actions}&#x60; with &#x60;code&#x60; &#x60;UNSUPPORTED&#x60;; the three cases differ in &#x60;error&#x60;. (1) No Model Plane URL: &#x60;structured extraction needs the Model Plane, and QUARRY_EDGE__MODEL_PLANE_URL is not set on this deployment&#x60;. (2) A URL but no model credential: &#x60;structured extraction needs a granted Model Plane lane, and this deployment holds no model credential&#x60;. (3) The gateway refused the first model call (answered after one fetch and one model call, only while no source was extracted): &#x60;structured extraction needs a granted Model Plane lane, and the Model Plane refused this service&#x60;. The retry is the same request without &#x60;schema&#x60; and &#x60;prompt&#x60; (cleaned markdown). A deployed stack gets one of these while the &#x60;model-invoke&#x60; row stays &#x60;granted: false&#x60; (decision D16, a Quarry-side gate); no flag unlocks it. See RUNBOOKS, \&quot;Enable structured extract and AI formats\&quot;. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

