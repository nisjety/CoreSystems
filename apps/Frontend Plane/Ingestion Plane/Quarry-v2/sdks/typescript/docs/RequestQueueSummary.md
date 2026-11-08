
# RequestQueueSummary


## Properties

Name | Type
------------ | -------------
`queueId` | string
`orgId` | string
`name` | string
`kind` | string
`status` | string
`createdAt` | Date
`stats` | [RequestQueueSummaryStats](RequestQueueSummaryStats.md)

## Example

```typescript
import type { RequestQueueSummary } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "queueId": null,
  "orgId": null,
  "name": null,
  "kind": null,
  "status": null,
  "createdAt": null,
  "stats": null,
} satisfies RequestQueueSummary

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as RequestQueueSummary
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


