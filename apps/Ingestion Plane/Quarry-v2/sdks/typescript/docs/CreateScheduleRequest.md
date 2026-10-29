
# CreateScheduleRequest


## Properties

Name | Type
------------ | -------------
`name` | string
`kind` | string
`cron` | string
`scheduleAt` | Date
`overlapPolicy` | [OverlapPolicy](OverlapPolicy.md)
`catchupWindowS` | number
`pauseOnFailure` | boolean
`preset` | string
`targetRef` | string
`config` | { [key: string]: any; }

## Example

```typescript
import type { CreateScheduleRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "name": null,
  "kind": null,
  "cron": null,
  "scheduleAt": null,
  "overlapPolicy": null,
  "catchupWindowS": null,
  "pauseOnFailure": null,
  "preset": null,
  "targetRef": null,
  "config": null,
} satisfies CreateScheduleRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CreateScheduleRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


