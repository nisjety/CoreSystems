# ScrapeApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**scrape**](ScrapeApi.md#scrapeoperation) | **POST** /v1/scrape | Scrape a single URL |
| [**scrapeStream**](ScrapeApi.md#scrapestream) | **POST** /v1/scrape/stream | Scrape with SSE event stream |



## scrape

> EnvelopeNormalizedOutput scrape(scrapeRequest)

Scrape a single URL

Fetches a URL, runs the transform pipeline (readability, markdown, links, metadata, fingerprint, diff), stores artifacts, and returns a normalized output envelope. Optionally forwards results to the Data Plane for ingestion. 

### Example

```ts
import {
  Configuration,
  ScrapeApi,
} from '@quarry/client';
import type { ScrapeOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ScrapeApi();

  const body = {
    // ScrapeRequest
    scrapeRequest: ...,
  } satisfies ScrapeOperationRequest;

  try {
    const data = await api.scrape(body);
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
| **scrapeRequest** | [ScrapeRequest](ScrapeRequest.md) |  | |

### Return type

[**EnvelopeNormalizedOutput**](EnvelopeNormalizedOutput.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Successful scrape |  -  |
| **400** | Invalid request (bad URL, missing fields) |  -  |
| **403** | Security blocked (SSRF, blocklist, policy) |  -  |
| **429** | Rate limited |  -  |
| **502** | Upstream fetch failed |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## scrapeStream

> string scrapeStream(scrapeRequest)

Scrape with SSE event stream

Same as /v1/scrape but returns a Server-Sent Event stream with per-step events (page_fetched, change_detected, artifact_written, etc.) followed by the final result or error. 

### Example

```ts
import {
  Configuration,
  ScrapeApi,
} from '@quarry/client';
import type { ScrapeStreamRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ScrapeApi();

  const body = {
    // ScrapeRequest
    scrapeRequest: ...,
  } satisfies ScrapeStreamRequest;

  try {
    const data = await api.scrapeStream(body);
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
| **scrapeRequest** | [ScrapeRequest](ScrapeRequest.md) |  | |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `text/event-stream`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | SSE event stream |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

