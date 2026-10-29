
# QueuedResponse

Acknowledgement for async/queued AI modality requests.

## Properties

Name | Type
------------ | -------------
`id` | string
`status` | string
`note` | string

## Example

```typescript
import type { QueuedResponse } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "status": null,
  "note": null,
} satisfies QueuedResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as QueuedResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


