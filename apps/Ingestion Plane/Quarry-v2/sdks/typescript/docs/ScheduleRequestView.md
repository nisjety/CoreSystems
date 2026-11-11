
# ScheduleRequestView

One trigger or backfill request row as the tenant reads it (WS15.S3a). It never shows the worker or the lease.

## Properties

Name | Type
------------ | -------------
`requestId` | string
`scheduleId` | string
`action` | string
`status` | string
`attempts` | number
`params` | { [key: string]: any; }
`result` | { [key: string]: any; }
`error` | string
`createdAt` | number
`finishedAt` | number

## Example

```typescript
import type { ScheduleRequestView } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "requestId": null,
  "scheduleId": null,
  "action": null,
  "status": null,
  "attempts": null,
  "params": null,
  "result": null,
  "error": null,
  "createdAt": null,
  "finishedAt": null,
} satisfies ScheduleRequestView

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScheduleRequestView
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


