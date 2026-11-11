
# JobStatusView

The status document of a crawl, batch or other durable job (WS15.L1).

## Properties

Name | Type
------------ | -------------
`jobId` | string
`kind` | string
`status` | string
`phase` | string
`terminal` | boolean
`origin` | [JobRunOrigin](JobRunOrigin.md)
`createdAt` | Date
`finishedAt` | Date
`deadlineAt` | Date
`counts` | [JobStatusViewCounts](JobStatusViewCounts.md)
`total` | number
`totalKind` | string
`warnings` | [Array&lt;JobStatusViewWarningsInner&gt;](JobStatusViewWarningsInner.md)
`control` | [JobStatusViewControl](JobStatusViewControl.md)
`scheduleId` | string
`runId` | string
`credits` | [JobCredits](JobCredits.md)

## Example

```typescript
import type { JobStatusView } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "jobId": null,
  "kind": null,
  "status": null,
  "phase": null,
  "terminal": null,
  "origin": null,
  "createdAt": null,
  "finishedAt": null,
  "deadlineAt": null,
  "counts": null,
  "total": null,
  "totalKind": null,
  "warnings": null,
  "control": null,
  "scheduleId": null,
  "runId": null,
  "credits": null,
} satisfies JobStatusView

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as JobStatusView
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


