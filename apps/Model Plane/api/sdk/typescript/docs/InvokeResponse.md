
# InvokeResponse


## Properties

Name | Type
------------ | -------------
`requestId` | string
`content` | string
`modelUsed` | string

## Example

```typescript
import type { InvokeResponse } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "requestId": null,
  "content": null,
  "modelUsed": null,
} satisfies InvokeResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as InvokeResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


