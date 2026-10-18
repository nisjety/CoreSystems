
# ScrapeResponse

A NormalizedOutput plus what cannot be a stored artifact. Under Zero Data Retention no artifact is written, so the extracted text is returned inline in `markdown`; otherwise read it from formats.markdown. A PDF or DOCX (QUARRY_EDGE__DOC_ROUTING=on) stores its original bytes as formats.raw and has no formats.html.

## Properties

Name | Type
------------ | -------------
`runId` | string
`url` | [UrlTriple](UrlTriple.md)
`status` | number
`fetchedAt` | Date
`fingerprint` | string
`formats` | [OutputFormats](OutputFormats.md)
`change` | [ChangeInfo](ChangeInfo.md)
`metadata` | [PageMetadata](PageMetadata.md)
`driver` | [DriverInfo](DriverInfo.md)
`markdown` | string
`html` | string
`rawHtml` | string
`truncated` | Array&lt;string&gt;
`omitted` | Array&lt;string&gt;
`effectiveOptions` | object
`warnings` | [Array&lt;ScrapeResponseAllOfWarnings&gt;](ScrapeResponseAllOfWarnings.md)

## Example

```typescript
import type { ScrapeResponse } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "runId": null,
  "url": null,
  "status": null,
  "fetchedAt": null,
  "fingerprint": null,
  "formats": null,
  "change": null,
  "metadata": null,
  "driver": null,
  "markdown": null,
  "html": null,
  "rawHtml": null,
  "truncated": null,
  "omitted": null,
  "effectiveOptions": null,
  "warnings": null,
} satisfies ScrapeResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScrapeResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


