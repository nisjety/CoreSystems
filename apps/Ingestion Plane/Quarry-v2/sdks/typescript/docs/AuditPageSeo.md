
# AuditPageSeo

The structural SEO evidence of the page (seo_page_v1). Interpreting it is the caller\'s job.

## Properties

Name | Type
------------ | -------------
`version` | string
`noindex` | boolean
`nofollow` | boolean
`robotsStatus` | number
`wordCount` | number
`hreflang` | [Array&lt;AuditPageAlternate&gt;](AuditPageAlternate.md)
`invalidAlternates` | number
`alternatesPartial` | boolean
`structuredData` | [AuditPageStructuredData](AuditPageStructuredData.md)

## Example

```typescript
import type { AuditPageSeo } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "version": null,
  "noindex": null,
  "nofollow": null,
  "robotsStatus": null,
  "wordCount": null,
  "hreflang": null,
  "invalidAlternates": null,
  "alternatesPartial": null,
  "structuredData": null,
} satisfies AuditPageSeo

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AuditPageSeo
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


