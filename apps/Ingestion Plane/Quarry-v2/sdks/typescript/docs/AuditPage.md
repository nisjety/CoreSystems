
# AuditPage

The bounded_page_v1 evidence of one public page (POST /v1/audit/page). No raw HTML, no artifact, no ingest and no durable storage. `metadata.title` and `metadata.description` are the page\'s own title and standard description tag, not Open Graph fallbacks. Links are capped at 100 and headings at 200, and `partial` is true when the source exceeds either cap.

## Properties

Name | Type
------------ | -------------
`version` | string
`url` | string
`finalUrl` | string
`status` | number
`fetchedAt` | Date
`runId` | string
`fingerprint` | string
`metadata` | [AuditPageMetadata](AuditPageMetadata.md)
`canonical` | string
`links` | [Array&lt;AuditPageLink&gt;](AuditPageLink.md)
`headings` | [Array&lt;AuditPageHeading&gt;](AuditPageHeading.md)
`seo` | [AuditPageSeo](AuditPageSeo.md)
`redirects` | [Array&lt;AuditPageRedirect&gt;](AuditPageRedirect.md)
`partial` | boolean
`limits` | [AuditPageLimits](AuditPageLimits.md)

## Example

```typescript
import type { AuditPage } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "version": null,
  "url": null,
  "finalUrl": null,
  "status": null,
  "fetchedAt": null,
  "runId": null,
  "fingerprint": null,
  "metadata": null,
  "canonical": null,
  "links": null,
  "headings": null,
  "seo": null,
  "redirects": null,
  "partial": null,
  "limits": null,
} satisfies AuditPage

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AuditPage
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


