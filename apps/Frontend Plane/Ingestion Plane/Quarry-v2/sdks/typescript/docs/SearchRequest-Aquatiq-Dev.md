
# SearchRequest


## Properties

Name | Type
------------ | -------------
`query` | string
`limit` | number
`country` | string
`language` | string
`safeSearch` | boolean
`topic` | string
`timeRange` | string
`days` | number
`exactMatch` | boolean
`chunksPerSource` | number
`includeAnswer` | boolean
`format` | string
`highlight` | boolean
`facets` | boolean

## Example

```typescript
import type { SearchRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "query": null,
  "limit": null,
  "country": null,
  "language": null,
  "safeSearch": null,
  "topic": null,
  "timeRange": null,
  "days": null,
  "exactMatch": null,
  "chunksPerSource": null,
  "includeAnswer": null,
  "format": null,
  "highlight": null,
  "facets": null,
} satisfies SearchRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SearchRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


