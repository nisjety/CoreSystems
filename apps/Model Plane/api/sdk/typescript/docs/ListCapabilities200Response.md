
# ListCapabilities200Response


## Properties

Name | Type
------------ | -------------
`capabilities` | [Array&lt;Capability&gt;](Capability.md)
`hasMore` | boolean

## Example

```typescript
import type { ListCapabilities200Response } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "capabilities": null,
  "hasMore": null,
} satisfies ListCapabilities200Response

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ListCapabilities200Response
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


