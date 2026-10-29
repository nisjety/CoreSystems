
# MapResponse


## Properties

Name | Type
------------ | -------------
`url` | string
`links` | [Array&lt;MapLink&gt;](MapLink.md)
`count` | number
`ranked` | boolean

## Example

```typescript
import type { MapResponse } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "links": null,
  "count": null,
  "ranked": null,
} satisfies MapResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as MapResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


