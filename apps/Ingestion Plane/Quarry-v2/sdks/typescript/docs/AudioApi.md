# AudioApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**audio**](AudioApi.md#audiooperation) | **POST** /v1/audio | Speech synthesis or transcription through the Model Plane |



## audio

> { [key: string]: any; } audio(audioRequest)

Speech synthesis or transcription through the Model Plane

One route with two shapes. A body with a string input is text to speech. A body with a string audio_url, and no string input, is transcription. Both are forwarded to the Model Plane, which does the work. Any other object is 400 BAD_REQUEST. The route is off unless QUARRY_EDGE__MODEL_AUDIO is set to on, and boot must also find a Model Plane path: QUARRY_EDGE__MODEL_PLANE_URL, a service identity, and a granted model-audio row in the cross-plane token-request manifest. Otherwise it answers 501 UNSUPPORTED, with no token mint and no Model Plane call. A transcription\&#39;s audio_url passes the web policy before anything leaves the edge; a refused URL is 403 and never reaches the token service or the Model Plane. Zero data retention: the edge keeps no copy, the request\&#39;s zdr is forwarded to the Model Plane (which owns retention for audio), and a token of a ZDR organisation raises it to true whatever the body says. A status the edge does not set itself is the Model Plane\&#39;s answer, passed through with its JSON body (an empty object when that body is not JSON). A service principal is refused with a bare 403.

### Example

```ts
import {
  Configuration,
  AudioApi,
} from '@quarry/client';
import type { AudioOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AudioApi();

  const body = {
    // AudioRequest
    audioRequest: ...,
  } satisfies AudioOperationRequest;

  try {
    const data = await api.audio(body);
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
| **audioRequest** | [AudioRequest](AudioRequest.md) |  | |

### Return type

**{ [key: string]: any; }**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The Model Plane\&#39;s JSON answer, unchanged |  -  |
| **400** | BAD_REQUEST when the object matches neither shape, including one whose fields have the wrong types. A body that is not JSON gets the framework\&#39;s plain-text 400. |  -  |
| **403** | FORBIDDEN when the web policy refuses the audio_url, with closed details that never carry the url |  -  |
| **422** | The body is JSON but not an object; the framework answers in plain text |  -  |
| **500** | INTERNAL when the HTTP client cannot be built or the service-token request manifest is unavailable |  -  |
| **501** | UNSUPPORTED while the switch is off or boot refuses it; no token is minted and no Model Plane call is made |  -  |
| **502** | UPSTREAM_BLOCKED when the Model Plane token cannot be minted; DRIVER_FAILED when the Model Plane cannot be reached |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

