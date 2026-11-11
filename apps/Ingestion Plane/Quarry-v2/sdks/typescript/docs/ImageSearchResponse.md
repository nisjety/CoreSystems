
# ImageSearchResponse


## Properties

Name | Type
------------ | -------------
`query` | string
`provider` | string
`images` | [Array&lt;ImageResult&gt;](ImageResult.md)

## Example

```typescript
import type { ImageSearchResponse } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "query": null,
  "provider": null,
  "images": null,
} satisfies ImageSearchResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ImageSearchResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


