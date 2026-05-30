
# OrchestrationEvent

SSE event from /v1/runs/{run_id}/events

## Properties

Name | Type
------------ | -------------
`eventType` | string
`data` | { [key: string]: any; }

## Example

```typescript
import type { OrchestrationEvent } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "eventType": null,
  "data": null,
} satisfies OrchestrationEvent

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as OrchestrationEvent
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


