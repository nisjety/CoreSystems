
# ScheduleRequestAccepted

The 202 answer of a trigger or backfill (WS15.S3a). The request is queued and not yet applied.

## Properties

Name | Type
------------ | -------------
`requestId` | string
`scheduleId` | string
`action` | string
`status` | string

## Example

```typescript
import type { ScheduleRequestAccepted } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "requestId": null,
  "scheduleId": null,
  "action": null,
  "status": null,
} satisfies ScheduleRequestAccepted

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScheduleRequestAccepted
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


