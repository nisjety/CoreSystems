
# SearchScrapeBlock

E3.7.7a. Hydrates the top results of this search through the edge\'s markdown fetch path, with the request\'s web policy and the security preflight applied to every URL before its fetch. Read only when QUARRY_EDGE__SEARCH_SCRAPE is on. Out-of-range values are refused with 400 BAD_REQUEST, not clamped. Hydrated text travels inline in this response only: it is never cached, never evented and never logged, under zero data retention too.

## Properties

Name | Type
------------ | -------------
`limit` | number
`maxChars` | number
`timeoutMs` | number
`formats` | Array&lt;string&gt;

## Example

```typescript
import type { SearchScrapeBlock } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "limit": null,
  "maxChars": null,
  "timeoutMs": null,
  "formats": null,
} satisfies SearchScrapeBlock

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SearchScrapeBlock
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


