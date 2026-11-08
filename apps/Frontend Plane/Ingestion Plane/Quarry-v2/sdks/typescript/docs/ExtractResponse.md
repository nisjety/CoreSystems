
# ExtractResponse


## Properties

Name | Type
------------ | -------------
`results` | [Array&lt;ExtractItem&gt;](ExtractItem.md)
`count` | number
`requested` | number

## Example

```typescript
import type { ExtractResponse } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "results": null,
  "count": null,
  "requested": null,
} satisfies ExtractResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ExtractResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


