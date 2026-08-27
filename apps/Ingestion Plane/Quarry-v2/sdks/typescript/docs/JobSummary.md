
# JobSummary


## Properties

Name | Type
------------ | -------------
`jobId` | string
`kind` | string
`orgId` | string
`status` | string
`createdAt` | Date
`runId` | string
`startedAt` | Date
`completedAt` | Date
`stats` | any

## Example

```typescript
import type { JobSummary } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "jobId": null,
  "kind": null,
  "orgId": null,
  "status": null,
  "createdAt": null,
  "runId": null,
  "startedAt": null,
  "completedAt": null,
  "stats": null,
} satisfies JobSummary

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as JobSummary
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


