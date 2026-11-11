
# ProcessingContinuesEnvelopeErrorDetails


## Properties

Name | Type
------------ | -------------
`reason` | string
`jobId` | string
`status` | string
`phase` | string
`counts` | [ProcessingContinuesEnvelopeErrorDetailsCounts](ProcessingContinuesEnvelopeErrorDetailsCounts.md)
`progressed` | boolean
`waitedS` | number
`retryAfterSeconds` | number
`resumeToken` | string

## Example

```typescript
import type { ProcessingContinuesEnvelopeErrorDetails } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "reason": null,
  "jobId": null,
  "status": null,
  "phase": null,
  "counts": null,
  "progressed": null,
  "waitedS": null,
  "retryAfterSeconds": null,
  "resumeToken": null,
} satisfies ProcessingContinuesEnvelopeErrorDetails

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProcessingContinuesEnvelopeErrorDetails
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


