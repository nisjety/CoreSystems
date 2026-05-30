
# InvokeRequest


## Properties

Name | Type
------------ | -------------
`content` | string
`model` | string
`sessionKey` | string
`threadId` | string
`structuredOutputSchema` | string
`zdr` | boolean
`maxCostUsd` | number
`maxTokens` | number

## Example

```typescript
import type { InvokeRequest } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "content": null,
  "model": null,
  "sessionKey": null,
  "threadId": null,
  "structuredOutputSchema": null,
  "zdr": null,
  "maxCostUsd": null,
  "maxTokens": null,
} satisfies InvokeRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as InvokeRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


