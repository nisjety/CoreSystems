
# Approval


## Properties

Name | Type
------------ | -------------
`id` | string
`runId` | string
`stepId` | string
`kind` | string
`state` | string
`requestedOf` | string
`decidedBy` | string
`decisionReason` | string

## Example

```typescript
import type { Approval } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "runId": null,
  "stepId": null,
  "kind": null,
  "state": null,
  "requestedOf": null,
  "decidedBy": null,
  "decisionReason": null,
} satisfies Approval

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Approval
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


