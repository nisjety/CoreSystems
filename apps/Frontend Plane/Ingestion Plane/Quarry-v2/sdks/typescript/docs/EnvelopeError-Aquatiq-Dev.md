
# EnvelopeError


## Properties

Name | Type
------------ | -------------
`requestId` | string
`success` | boolean
`error` | [EnvelopeErrorError](EnvelopeErrorError.md)

## Example

```typescript
import type { EnvelopeError } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "requestId": null,
  "success": null,
  "error": null,
} satisfies EnvelopeError

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as EnvelopeError
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


