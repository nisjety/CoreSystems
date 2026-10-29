
# JobHistoryEvent


## Properties

Name | Type
------------ | -------------
`runId` | string
`orgId` | string
`kind` | string
`stage` | [JobStage](JobStage.md)
`status` | string
`seq` | number
`completed` | number
`total` | number
`discovered` | number
`queued` | number
`retries` | number
`blocks` | number
`eta` | Date
`timestamp` | Date

## Example

```typescript
import type { JobHistoryEvent } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "runId": null,
  "orgId": null,
  "kind": null,
  "stage": null,
  "status": null,
  "seq": null,
  "completed": null,
  "total": null,
  "discovered": null,
  "queued": null,
  "retries": null,
  "blocks": null,
  "eta": null,
  "timestamp": null,
} satisfies JobHistoryEvent

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as JobHistoryEvent
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


