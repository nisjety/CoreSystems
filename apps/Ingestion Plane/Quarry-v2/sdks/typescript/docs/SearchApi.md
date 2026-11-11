# SearchApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**answer**](SearchApi.md#answeroperation) | **POST** /v1/answer | One-call grounded answer with citations |
| [**answerStream**](SearchApi.md#answerstream) | **POST** /v1/answer/stream | Server-sent events variant of /v1/answer |
| [**search**](SearchApi.md#searchoperation) | **POST** /v1/search | SERP + own-corpus search (hybrid when Data Plane wired) |
| [**searchImages**](SearchApi.md#searchimages) | **POST** /v1/search/images | Image search through the SearXNG image vertical |
| [**searchSimilar**](SearchApi.md#searchsimilar) | **POST** /v1/search/similar | Semantic neighbours of a passage, a page or both |
| [**searchSuggest**](SearchApi.md#searchsuggest) | **POST** /v1/search/suggest | Spelling correction, related searches and a panel for one query |
| [**searchVideos**](SearchApi.md#searchvideos) | **POST** /v1/search/videos | Video search through the SearXNG video vertical |



## answer

> AnswerResponse answer(answerRequest)

One-call grounded answer with citations

Searches, reads the top pages and synthesizes one answer through the Model Plane, with the sources as citations. Metered as &#x60;quarry.answer.synth&#x60;, once per synthesis. Off until the operator turns it on: it needs &#x60;QUARRY_EDGE__ANSWER_SYNTHESIS&#x3D;1&#x60; (decided by the Model Plane boot gate, which refuses a switch that has no granted model path), a search provider and &#x60;QUARRY_EDGE__MODEL_PLANE_URL&#x60;, and a Model Plane that accepts the edge\&#39;s service token. When it is not available the response names the reason, see &#x60;AnswerErrorBody&#x60;. The same pipeline serves &#x60;include_answer&#x60; on &#x60;POST /v1/search&#x60;, which reports the same reasons as &#x60;answer_status&#x60; and &#x60;answer_reason&#x60;. A zero data retention request (the body flag or the token\&#39;s floor) is refused with a 403, reason &#x60;zdr_unavailable&#x60;, before the pipeline starts: the model hop carries no ZDR posture, so there is no search, no page fetch and no Model Plane call. The refusal leaves a metadata-only tombstone when that is switched on.

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { AnswerOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // AnswerRequest
    answerRequest: ...,
  } satisfies AnswerOperationRequest;

  try {
    const data = await api.answer(body);
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
| **answerRequest** | [AnswerRequest](AnswerRequest.md) |  | |

### Return type

[**AnswerResponse**](AnswerResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The answer. With no usable source &#x60;answer&#x60; is empty and &#x60;model&#x60; is empty; no model was called. |  -  |
| **400** | The query is empty |  -  |
| **403** | Zero data retention (the body flag or the token\&#39;s floor): refused before any work, &#x60;code&#x60; &#x60;FORBIDDEN&#x60;, &#x60;reason&#x60; &#x60;zdr_unavailable&#x60;. The Model Plane hop carries no ZDR posture, so nothing is searched, fetched or sent to a model. |  -  |
| **501** | The answer pipeline is not wired. &#x60;reason&#x60; is the first closed gate: &#x60;synthesis_disabled&#x60;, &#x60;model_path_refused&#x60; (the switch is on and the boot gate refused it), &#x60;no_model_plane_url&#x60;, &#x60;no_search_provider&#x60; or &#x60;model_client_init_failed&#x60;; &#x60;hint&#x60; names the exact setting. |  -  |
| **502** | The Model Plane failed the call (&#x60;reason&#x60; &#x60;model_plane_error&#x60;), or the search or page-fetch step failed (&#x60;answer_search_failed&#x60;) |  -  |
| **503** | The pipeline is wired but the Model Plane refused the edge\&#39;s service token (&#x60;reason&#x60; &#x60;model_plane_auth_refused&#x60;). A deployment fact, not a fault of the caller; the caller\&#39;s own token is fine. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## answerStream

> string answerStream(answerRequest)

Server-sent events variant of /v1/answer

Same pipeline and same request. Frames: &#x60;event: citations&#x60; (the citation array), &#x60;event: delta&#x60; (&#x60;{\&quot;delta\&quot;: text}&#x60;, repeated), then &#x60;event: done&#x60; (&#x60;{\&quot;answer\&quot;, \&quot;cached\&quot;}&#x60;); &#x60;event: error&#x60; (&#x60;{\&quot;message\&quot;, \&quot;reason\&quot;}&#x60;) on failure, where &#x60;reason&#x60; is the closed code of &#x60;AnswerErrorBody&#x60;. A warm org-scoped answer-cache hit replays with no model call. A ZDR request is refused with the same JSON 403 as &#x60;/v1/answer&#x60; (reason &#x60;zdr_unavailable&#x60;), not a stream. When the pipeline is not wired the response is the same 501 as &#x60;/v1/answer&#x60;, not a stream. Metered as &#x60;quarry.answer.synth&#x60; when an answer is produced.

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { AnswerStreamRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // AnswerRequest
    answerRequest: ...,
  } satisfies AnswerStreamRequest;

  try {
    const data = await api.answerStream(body);
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
| **answerRequest** | [AnswerRequest](AnswerRequest.md) |  | |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `text/event-stream`, `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | SSE stream of citations, deltas and a final &#x60;done&#x60; |  -  |
| **400** | The query is empty |  -  |
| **403** | Zero data retention; refused before the stream starts, &#x60;reason&#x60; &#x60;zdr_unavailable&#x60; |  -  |
| **501** | The answer pipeline is not wired; &#x60;reason&#x60; as for &#x60;/v1/answer&#x60; |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## search

> SearchResponse search(searchRequest)

SERP + own-corpus search (hybrid when Data Plane wired)

Provider-agnostic search across the local Tantivy corpus, SERP backends, and — when a Data Plane is configured — semantic vectors fused via RRF. Tavily-style params: topic, time_range/days, exact_match, chunks_per_source, include_answer, format&#x3D;context, highlight, facets. 

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { SearchOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // SearchRequest
    searchRequest: ...,
  } satisfies SearchOperationRequest;

  try {
    const data = await api.search(body);
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
| **searchRequest** | [SearchRequest](SearchRequest.md) |  | |

### Return type

[**SearchResponse**](SearchResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Ranked results (plus optional answer/context/facets; score_kind and rerank when a rerank ran) |  -  |
| **400** | Refused before any search runs. ZDR_REQUIRED: a service caller did not send zdr. SCORE_KIND_UNSUPPORTED: score_kinds names a score kind this edge does not produce. BAD_REQUEST: a scrape limit (1 to 10), max_chars (1 to 50000) or timeout_ms (1000 to 55000) is out of range. UNSUPPORTED_OPTION: a scrape formats entry other than markdown. These checks run only with QUARRY_EDGE__SEARCH_SCRAPE on; off, the scrape block is ignored. |  -  |
| **429** | Rate limited (structured retry envelope) |  -  |
| **501** | No search backend configured |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## searchImages

> ImageSearchResponse searchImages(imageSearchRequest)

Image search through the SearXNG image vertical

Focused image search. The query goes to the SearXNG the edge is configured with (QUARRY_EDGE__SEARXNG_URL), in its image category, and nothing else. Without that URL the route answers 501 UNSUPPORTED. A successful call, even one with no images, is metered as one SEARCH_QUERY unit with kind images. Zero data retention: the route reads no zdr field and makes no ZDR decision. The query is not cached, and the usage event carries the org, the user, the vertical and the result count, never the query text. Codes from SearXNG are the upper-cased names of the error code (DRIVERFAILED, BADREQUEST, NOTFOUND), not the underscore wire names; the 429 envelope carries RATE_LIMITED. A service principal is refused with a bare 403. A body that is not JSON, or does not match the schema, gets the framework\&#39;s plain-text 400 or 422 before the handler runs.

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { SearchImagesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // ImageSearchRequest
    imageSearchRequest: ...,
  } satisfies SearchImagesRequest;

  try {
    const data = await api.searchImages(body);
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
| **imageSearchRequest** | [ImageSearchRequest](ImageSearchRequest.md) |  | |

### Return type

[**ImageSearchResponse**](ImageSearchResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Image hits in the provider order, with the query as sent |  -  |
| **400** | BAD_REQUEST when the query is empty after trimming, or BADREQUEST when SearXNG answers another 4xx |  -  |
| **403** | FORBIDDEN when SearXNG answers 401 or 403 |  -  |
| **422** | The body does not match the schema (no query, or a wrong type); the framework answers in plain text |  -  |
| **429** | RATE_LIMITED when SearXNG answers 429, with the structured retry envelope |  -  |
| **500** | INTERNAL when the SearXNG client cannot be built; NOTFOUND when SearXNG answers 404 |  -  |
| **501** | UNSUPPORTED when no SearXNG URL is configured; the hint names QUARRY_EDGE__SEARXNG_URL |  -  |
| **502** | DRIVERFAILED when SearXNG cannot be reached, answers a 5xx, or sends a body that does not decode |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## searchSimilar

> SearchResponse searchSimilar(similarRequest)

Semantic neighbours of a passage, a page or both

Find-similar, from the Data Plane vector index. Send text (a passage), url (a page, whose markdown seeds the search) or both. A body with neither after trimming is 400 BAD_REQUEST. The route needs the Data Plane vector leg: QUARRY_EDGE__VECTOR_RETRIEVAL set to true, QUARRY_EDGE__DATA_PLANE_URL, and a service identity. Without it the route answers 501 UNSUPPORTED. A url is checked against the web policy before anything is fetched, even when text is also sent; a refusal is 403 FORBIDDEN, and its message never carries the url. The page is fetched only when text is absent, and when that fetch yields nothing the url itself seeds the search. The response query is the literal string text when a passage seeded the search, and the url otherwise. Results come in the Data Plane\&#39;s order with no score, and provider is similar. Each call that runs the vector leg is metered as one SEARCH_QUERY unit with kind similar. Zero data retention: there is no zdr field, so the posture is the token\&#39;s ZDR floor alone. Under ZDR the vector leg does not run (no Data Plane call, no hits, no meter) and the answer is 200 with no results; a url is still fetched under the ZDR posture first. Codes from the vector leg are the upper-cased names of the error code (UPSTREAMBLOCKED, DRIVERFAILED, RATELIMITED, TIMEOUT), not the underscore wire names. A service principal is refused with a bare 403. A body that is not JSON gets the framework\&#39;s plain-text 400.

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { SearchSimilarRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // SimilarRequest
    similarRequest: ...,
  } satisfies SearchSimilarRequest;

  try {
    const data = await api.searchSimilar(body);
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
| **similarRequest** | [SimilarRequest](SimilarRequest.md) |  | |

### Return type

[**SearchResponse**](SearchResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The neighbours, in the Data Plane\&#39;s order, with provider similar and no score |  -  |
| **400** | BAD_REQUEST when neither text nor url is non-empty after trimming |  -  |
| **403** | FORBIDDEN when the web policy refuses the url, or the token service refuses the Data Plane call |  -  |
| **422** | The body does not match the schema (a wrong type); the framework answers in plain text |  -  |
| **429** | RATELIMITED when the Data Plane or the token service answers 429; a plain error body, not the retry envelope |  -  |
| **500** | INTERNAL, or an upstream code the route does not map |  -  |
| **501** | UNSUPPORTED while the vector leg is not wired; the hint names the two settings |  -  |
| **502** | UPSTREAMBLOCKED or DRIVERFAILED when the Data Plane or the token service refuses, fails or cannot be reached |  -  |
| **504** | TIMEOUT when the Data Plane or the token service misses its deadline |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## searchSuggest

> SuggestResponse searchSuggest(suggestRequest)

Spelling correction, related searches and a panel for one query

Did-you-mean, related searches and a knowledge panel for one query, from the Model Plane. The route is degrade-safe and always answers 200: with nothing to give, the answer is an object whose related_queries is empty. It is off unless QUARRY_EDGE__SEARCH_SUGGEST is set to on, and boot must also find a Model Plane path (QUARRY_EDGE__MODEL_PLANE_URL, a service identity and a granted model-invoke row). When the switch is off, or boot refuses it, the route answers the same empty 200 and makes no Model Plane call; so does an empty query. The model\&#39;s reply is read leniently: a correction equal to the query is dropped, related queries are trimmed, de-duplicated and capped at five, and a reply that does not parse gives no suggestions. Zero data retention is not honoured: the handler reads no ZDR posture and the Model Plane call carries none, so under ZDR the query still reaches the model whenever the route runs. The edge keeps no copy. A service principal is refused with a bare 403. A body that is not JSON, or does not match the schema, gets the framework\&#39;s plain-text 400 or 422.

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { SearchSuggestRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // SuggestRequest
    suggestRequest: ...,
  } satisfies SearchSuggestRequest;

  try {
    const data = await api.searchSuggest(body);
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
| **suggestRequest** | [SuggestRequest](SuggestRequest.md) |  | |

### Return type

[**SuggestResponse**](SuggestResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The suggestions for the query, or the empty answer |  -  |
| **400** | The body is not JSON (the framework\&#39;s plain-text rejection) |  -  |
| **422** | The body does not match the schema (no query, or a wrong type); the framework answers in plain text |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## searchVideos

> VideoSearchResponse searchVideos(videoSearchRequest)

Video search through the SearXNG video vertical

Focused video search. The query goes to the SearXNG the edge is configured with (QUARRY_EDGE__SEARXNG_URL), in its video category, and nothing else. Without that URL the route answers 501 UNSUPPORTED. A successful call, even one with no videos, is metered as one SEARCH_QUERY unit with kind videos. Zero data retention: the route reads no zdr field and makes no ZDR decision. The query is not cached, and the usage event carries the org, the user, the vertical and the result count, never the query text. Codes from SearXNG are the upper-cased names of the error code (DRIVERFAILED, BADREQUEST, NOTFOUND), not the underscore wire names; the 429 envelope carries RATE_LIMITED. A service principal is refused with a bare 403. A body that is not JSON, or does not match the schema, gets the framework\&#39;s plain-text 400 or 422 before the handler runs.

### Example

```ts
import {
  Configuration,
  SearchApi,
} from '@quarry/client';
import type { SearchVideosRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SearchApi();

  const body = {
    // VideoSearchRequest
    videoSearchRequest: ...,
  } satisfies SearchVideosRequest;

  try {
    const data = await api.searchVideos(body);
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
| **videoSearchRequest** | [VideoSearchRequest](VideoSearchRequest.md) |  | |

### Return type

[**VideoSearchResponse**](VideoSearchResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Video hits in the provider order, with the query as sent |  -  |
| **400** | BAD_REQUEST when the query is empty after trimming, or BADREQUEST when SearXNG answers another 4xx |  -  |
| **403** | FORBIDDEN when SearXNG answers 401 or 403 |  -  |
| **422** | The body does not match the schema (no query, or a wrong type); the framework answers in plain text |  -  |
| **429** | RATE_LIMITED when SearXNG answers 429, with the structured retry envelope |  -  |
| **500** | INTERNAL when the SearXNG client cannot be built; NOTFOUND when SearXNG answers 404 |  -  |
| **501** | UNSUPPORTED when no SearXNG URL is configured; the hint names QUARRY_EDGE__SEARXNG_URL |  -  |
| **502** | DRIVERFAILED when SearXNG cannot be reached, answers a 5xx, or sends a body that does not decode |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

