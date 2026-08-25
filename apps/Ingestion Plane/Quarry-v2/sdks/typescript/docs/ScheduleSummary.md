
# ScheduleSummary


## Properties

Name | Type
------------ | -------------
`scheduleId` | string
`orgId` | string
`name` | string
`kind` | string
`cron` | string
`scheduleAt` | Date
`overlapPolicy` | [OverlapPolicy](OverlapPolicy.md)
`catchupWindowS` | number
`pauseOnFailure` | boolean
`status` | string
`createdAt` | Date
`lastRunAt` | Date
`nextRunAt` | Date
`config` | any

## Example

```typescript
import type { ScheduleSummary } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "scheduleId": null,
  "orgId": null,
  "name": null,
  "kind": null,
  "cron": null,
  "scheduleAt": null,
  "overlapPolicy": null,
  "catchupWindowS": null,
  "pauseOnFailure": null,
  "status": null,
  "createdAt": null,
  "lastRunAt": null,
  "nextRunAt": null,
  "config": null,
} satisfies ScheduleSummary

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScheduleSummary
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


