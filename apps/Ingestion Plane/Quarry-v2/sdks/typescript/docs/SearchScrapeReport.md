
# SearchScrapeReport

The scrape block\'s result. It is present only when the request sent a scrape block. With the flag off the status is disabled and nothing was fetched.

## Properties

Name | Type
------------ | -------------
`status` | string
`requested` | number
`hydrated` | number
`failed` | number
`items` | [Array&lt;SearchScrapeItem&gt;](SearchScrapeItem.md)

## Example

```typescript
import type { SearchScrapeReport } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "status": null,
  "requested": null,
  "hydrated": null,
  "failed": null,
  "items": null,
} satisfies SearchScrapeReport

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SearchScrapeReport
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


