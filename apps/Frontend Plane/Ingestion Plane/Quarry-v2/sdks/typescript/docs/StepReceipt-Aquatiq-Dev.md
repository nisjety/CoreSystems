
# StepReceipt


## Properties

Name | Type
------------ | -------------
`orgId` | string
`receiptId` | string
`runId` | string
`step` | number
`startedAt` | Date
`finishedAt` | Date
`action` | { [key: string]: any; }
`outcome` | { [key: string]: any; }
`observation` | [BrowserObservation](BrowserObservation.md)
`correctionOf` | string
`costMicroUsd` | number

## Example

```typescript
import type { StepReceipt } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "orgId": null,
  "receiptId": null,
  "runId": null,
  "step": null,
  "startedAt": null,
  "finishedAt": null,
  "action": null,
  "outcome": null,
  "observation": null,
  "correctionOf": null,
  "costMicroUsd": null,
} satisfies StepReceipt

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as StepReceipt
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


