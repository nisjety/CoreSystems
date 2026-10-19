# ScrapeApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**getScrape**](ScrapeApi.md#getscrape) | **GET** /v1/scrape/{id} | Read back an earlier scrape |
| [**scrape**](ScrapeApi.md#scrapeoperation) | **POST** /v1/scrape | Scrape a single URL |
| [**scrapeStream**](ScrapeApi.md#scrapestream) | **POST** /v1/scrape/stream | Scrape with SSE event stream |



## getScrape

> EnvelopeScrapeResponse getScrape(id)

Read back an earlier scrape

Returns the record a non-ZDR scrape stored: the normalized output and the effective options, without inline text. The id is the &#x60;scrape_id&#x60; of the scrape response. Another organization\&#39;s id, or an id that is not a scrape record, is a 404. A caller whose organization is under ZDR is refused with 403, even for a record written before the organization turned ZDR on. A ZDR scrape stores nothing and has no id.

### Example

```ts
import {
  Configuration,
  ScrapeApi,
} from '@quarry/client';
import type { GetScrapeRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ScrapeApi();

  const body = {
    // string | The scrape_id from a scrape response.
    id: id_example,
  } satisfies GetScrapeRequest;

  try {
    const data = await api.getScrape(body);
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
| **id** | `string` | The scrape_id from a scrape response. | [Defaults to `undefined`] |

### Return type

[**EnvelopeScrapeResponse**](EnvelopeScrapeResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The stored scrape |  * Cache-Control - private, max-age&#x3D;30 <br>  |
| **403** | The caller\&#39;s organization is under Zero Data Retention |  -  |
| **404** | No such scrape for this organization |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## scrape

> EnvelopeScrapeResponse scrape(scrapeRequest)

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

[**EnvelopeScrapeResponse**](EnvelopeScrapeResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Successful scrape |  -  |
| **415** | The page is a kind of file Quarry cannot extract text from (image, archive, spreadsheet, ...), or a PDF/DOCX that could not be read. Code UNSUPPORTED_MEDIA; details name the kind and, for a document that failed to parse, a reason (pdf_parse_failed, docx_parse_failed, too_large). Only returned when QUARRY_EDGE__DOC_ROUTING is skip or on. |  -  |
| **400** | Invalid request (bad URL, missing fields) |  -  |
| **403** | Security blocked (SSRF, blocklist, policy), or the fetched page is itself a bot challenge. A challenge is code CHALLENGE_DETECTED with details.block_reason (captcha_widget, waf_interstitial or unexpected_bot_error; the same value is given as details.reason), details.recovery not_applicable and details.next other_source. It carries no host, URL or page text. A status alone (a bare 403, a 429, a login page) is never a challenge, and a page that is not a challenge scrapes as before. Classified by QUARRY_CHALLENGE_CLASSIFIER&#x3D;v2 (the default; legacy does not classify a scrape). The refusal itself is switched on with QUARRY_CHALLENGE_ENFORCE&#x3D;enforce; under the default log the page is returned as before and only counted by reason in a log line. |  -  |
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

