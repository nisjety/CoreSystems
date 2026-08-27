
# TeamQueueStatus


## Properties

Name | Type
------------ | -------------
`orgId` | string
`queuedTotal` | number
`inFlightTotal` | number
`byQueue` | [Array&lt;QueueStatusEntry&gt;](QueueStatusEntry.md)

## Example

```typescript
import type { TeamQueueStatus } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "orgId": null,
  "queuedTotal": null,
  "inFlightTotal": null,
  "byQueue": null,
} satisfies TeamQueueStatus

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as TeamQueueStatus
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


