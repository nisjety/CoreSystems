
# Plan


## Properties

Name | Type
------------ | -------------
`id` | string
`runId` | string
`threadId` | string
`author` | string
`state` | [PlanState](PlanState.md)
`summary` | string
`steps` | [Array&lt;PlanStep&gt;](PlanStep.md)
`supersedes` | string

## Example

```typescript
import type { Plan } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "id": null,
  "runId": null,
  "threadId": null,
  "author": null,
  "state": null,
  "summary": null,
  "steps": null,
  "supersedes": null,
} satisfies Plan

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Plan
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


