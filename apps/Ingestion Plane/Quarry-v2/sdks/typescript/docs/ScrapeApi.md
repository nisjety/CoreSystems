# ScrapeApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**auditPage**](ScrapeApi.md#auditpageoperation) | **POST** /v1/audit/page | Bounded public-page evidence for the audit rules |
| [**getScrape**](ScrapeApi.md#getscrape) | **GET** /v1/scrape/{id} | Read back an earlier scrape |
| [**scrape**](ScrapeApi.md#scrapeoperation) | **POST** /v1/scrape | Scrape a single URL |
| [**scrapeStream**](ScrapeApi.md#scrapestream) | **POST** /v1/scrape/stream | Scrape with SSE event stream |



## auditPage

> AuditPage auditPage(auditPageRequest)

Bounded public-page evidence for the audit rules

Bounded public-page evidence (&#x60;bounded_page_v1&#x60;) for Marketing Core\&#39;s audit rules. It reads &#x60;robots.txt&#x60; for the page\&#39;s origin first, then the page, following redirects only inside the discovery scope with every hop traced into &#x60;redirects&#x60;. It runs no analysis and stores nothing, and calls no browser fallback, provider or ingest. The URL passes the web policy before any fetch, as &#x60;POST /v1/map&#x60; does, and a refusal names the layer that decided, not the URL. A 2xx source must be an HTML document (text/html or application/xhtml+xml), or the answer is 503 UNSUPPORTED. An upstream 404 or 500 is evidence in &#x60;status&#x60;, not a failure. A title or description over its limit is 502 UPSTREAM_LIMIT, never a truncated value. Link and heading text is cut at 512 characters, and the lists are capped at 100 and 200, with &#x60;partial&#x60; set when the source has more. The success body is the &#x60;AuditPage&#x60; object itself, not wrapped in the typed envelope. A service principal needs scrape:read (a bare 403 otherwise); a person\&#39;s token passes this gate. At most eight captures run per process, and an excess request is 429 RATE_LIMITED without queueing. The whole capture has a 10 second deadline, past which the answer is 504 TIMEOUT.

### Example

```ts
import {
  Configuration,
  ScrapeApi,
} from '@quarry/client';
import type { AuditPageOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ScrapeApi();

  const body = {
    // AuditPageRequest
    auditPageRequest: ...,
  } satisfies AuditPageOperationRequest;

  try {
    const data = await api.auditPage(body);
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
| **auditPageRequest** | [AuditPageRequest](AuditPageRequest.md) |  | |

### Return type

[**AuditPage**](AuditPage.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The bounded evidence for the page |  -  |
| **400** | The body is not JSON (the framework\&#39;s plain-text rejection) |  -  |
| **403** | Refused by the web policy, not a public URL, blocked, or with an unreadable robots.txt (FORBIDDEN) |  -  |
| **422** | The body does not match the schema (an unknown field, or a wrong type); the framework answers in plain text |  -  |
| **429** | At most eight captures are in flight, or the discovery transport is rate limited (RATE_LIMITED) |  -  |
| **502** | The evidence exceeds a field limit, or the source exceeds the bounded response limit (UPSTREAM_LIMIT) |  -  |
| **503** | The source is a 2xx document that is not HTML, or the bounded transport is unavailable (UNSUPPORTED) |  -  |
| **504** | The capture passed its 10 second deadline (TIMEOUT) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


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
| **403** | Security blocked (SSRF, blocklist, policy), or the fetched page is itself a bot challenge. A challenge is code CHALLENGE_DETECTED with details.block_reason (captcha_widget, waf_interstitial or unexpected_bot_error; the same value is given as details.reason), details.recovery not_applicable and details.next other_source. It carries no host, URL or page text. A status alone (a bare 403, a 429, a login page) is never a challenge, and a page that is not a challenge scrapes as before. Classified by QUARRY_CHALLENGE_CLASSIFIER&#x3D;v2 (the default; legacy does not classify a scrape). The refusal itself is switched on with QUARRY_CHALLENGE_ENFORCE&#x3D;enforce; under the default log the page is returned as before and only counted by reason in a log line. An AI format (json, summary, question, highlights) asked for by a service principal without the extract:ai scope, or on a request labelled initiated_by \&quot;agent\&quot;, is code FORBIDDEN with details.reason extract_ai_scope_required and details.field formats[i]; nothing is fetched. |  -  |
| **429** | Rate limited. With actions, details.reason heavy_browser_busy means the browser\&#39;s heavy lane is held by another action run; retry shortly. |  -  |
| **501** | The browser cannot do what was asked: a capture or actions on a browser that cannot run them, or no browser wired on the endpoint. Code UNSUPPORTED, details.reason browser_capability_unavailable. Also code UNSUPPORTED for an AI format when the endpoint has no model: details.reason model_plane_unavailable (QUARRY_EDGE__MODEL_PLANE_URL is unset) or model_lane_not_granted (no usable model credential on this deployment). Answered before the page is fetched. |  -  |
| **504** | The run hit a deadline. Without actions: code TIMEOUT with details.op scrape and deadline_ms. With actions (the handler bound is 115 s and the route allows 125 s): details.reason actions_deadline. A single wait over its limit (wait_too_long) or a run over the wait budget (wait_budget_exhausted) fails that action; under actions_on_error abort the request then answers 502 action_failed. |  -  |
| **502** | Upstream fetch failed. With actions, an action that failed under abort answers DRIVER_FAILED with details.reason action_failed, details.action_index, details.action_type and details.actions: the outcome of each action in order (index, kind, status, duration_ms, and an error code and message). Page text and typed values are never in it. details.reason no_browser_session means the run held no browser session. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## scrapeStream

> string scrapeStream(scrapeRequest)

Scrape with SSE event stream

Same as /v1/scrape but returns a Server-Sent Event stream with per-step events (page_fetched, change_detected, artifact_written, etc.) followed by the final result or error. Captures, actions, the AI formats and a profile pin (profile_id) are not served here: the error event is BAD_REQUEST with details.reason unsupported_in_stream. 

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

