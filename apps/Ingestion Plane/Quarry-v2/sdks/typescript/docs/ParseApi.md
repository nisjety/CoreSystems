# ParseApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**parseDocument**](ParseApi.md#parsedocument) | **POST** /v1/parse | Parse an uploaded PDF or DOCX to text and store nothing (INNH-11) |



## parseDocument

> ParseResponse parseDocument(body)

Parse an uploaded PDF or DOCX to text and store nothing (INNH-11)

The request body is the document\&#39;s raw bytes, not a URL and not multipart (a URL is &#x60;POST /v1/scrape&#x60; with QUARRY_EDGE__DOC_ROUTING&#x3D;on). The answer is the document\&#39;s text in the typed envelope. Nothing is stored: no artifact, no cache entry, no index row, no durable event, no usage record, no ingest; the response carries &#x60;Cache-Control: no-store&#x60;, and no filename or text is logged. The route is idempotent and has no side effect.  The bytes decide the kind (PDF or DOCX); &#x60;Content-Type&#x60; is only checked. It may be absent, &#x60;application/octet-stream&#x60;, &#x60;application/pdf&#x60; or the DOCX media type; any other type is a 415, and a PDF or DOCX type that the bytes contradict is a 415 &#x60;content_type_mismatch&#x60;. The body limit is 25 MiB (QUARRY_EDGE__PARSE_MAX_BYTES lowers it). The parse runs in an isolated child process under memory and CPU limits with a 15 second deadline. An organisation may have one parse in flight (QUARRY_EDGE__PARSE_CONCURRENCY_PER_ORG); a second gets a 429 with &#x60;Retry-After&#x60; before its body is read.  The text is plain (&#x60;structure: plain&#x60;): paragraphs, no headings, lists or tables. A PDF page with no text layer (a scan) has no text and a &#x60;document_has_no_text_layer&#x60; warning (or &#x60;some_pages_have_no_text_layer&#x60;); there is no OCR. No redaction is applied to the returned text: personal data in the document comes back as it is.  Dark: with QUARRY_EDGE__PARSE_ROUTE off (the default) the route answers 501. A service principal needs the &#x60;parse:write&#x60; scope; a user token needs none.

### Example

```ts
import {
  Configuration,
  ParseApi,
} from '@quarry/client';
import type { ParseDocumentRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ParseApi();

  const body = {
    // Blob
    body: BINARY_DATA_HERE,
  } satisfies ParseDocumentRequest;

  try {
    const data = await api.parseDocument(body);
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
| **body** | `Blob` |  | |

### Return type

[**ParseResponse**](ParseResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/pdf`, `application/vnd.openxmlformats-officedocument.wordprocessingml.document`, `application/octet-stream`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The text of the document. |  * Cache-Control -  <br>  |
| **400** | An empty body (&#x60;details.reason&#x60; &#x60;empty_body&#x60;), or a body that could not be read. |  -  |
| **401** | No valid token. |  -  |
| **403** | A service principal without the &#x60;parse:write&#x60; scope. |  -  |
| **413** | The body is over the limit (&#x60;details.reason&#x60; &#x60;too_large&#x60;, &#x60;details.max_bytes&#x60;), by its declared length or by its bytes. |  -  |
| **415** | Not a document the route reads. &#x60;details.reason&#x60; is &#x60;unsupported_media&#x60; (the bytes are not a PDF or DOCX, or the declared type is not one the route takes), &#x60;content_type_mismatch&#x60; (the declared type names the other document type than the bytes), &#x60;pdf_parse_failed&#x60; or &#x60;docx_parse_failed&#x60; (the parser could not read the file, or was stopped by its limits), or &#x60;too_large&#x60; (the parser\&#39;s own size or inflate limit). &#x60;details.supported&#x60; is &#x60;[pdf, docx]&#x60;. |  -  |
| **429** | The organisation already has a document being parsed (&#x60;org_parse_busy&#x60;), the process is buffering as many uploads as it will (&#x60;parse_capacity&#x60;), or the parser stayed busy (&#x60;parser_busy&#x60;). &#x60;Retry-After&#x60; says when to come back. |  * Retry-After -  <br>  |
| **501** | Off (&#x60;details.reason&#x60; &#x60;parse_route_disabled&#x60;, &#x60;details.env&#x60; QUARRY_EDGE__PARSE_ROUTE), or on while documents would be parsed inside the edge (&#x60;isolation_required&#x60;). |  -  |
| **504** | The parse did not finish in 15 seconds (&#x60;TIMEOUT&#x60;). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

