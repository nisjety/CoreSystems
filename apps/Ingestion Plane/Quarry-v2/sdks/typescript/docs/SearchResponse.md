
# SearchResponse


## Properties

Name | Type
------------ | -------------
`query` | string
`provider` | string
`results` | [Array&lt;SearchResultItem&gt;](SearchResultItem.md)
`count` | number
`answer` | string
`citations` | Array&lt;object&gt;
`context` | string
`facets` | [Array&lt;FacetCount&gt;](FacetCount.md)

## Example

```typescript
import type { SearchResponse } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "query": null,
  "provider": null,
  "results": null,
  "count": null,
  "answer": null,
  "citations": null,
  "context": null,
  "facets": null,
} satisfies SearchResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SearchResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


