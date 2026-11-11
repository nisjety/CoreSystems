
# ProcessingContinuesEnvelope

The 408 of a job read that waited (wait, with QUARRY_EDGE__JOB_CONTROL on) and ran out before the job reached a terminal status. It is the typed error envelope of the job routes, with data null. The HTTP status is 408 where TIMEOUT is 504 everywhere else, so a client keys on the HTTP 408 and on error.details.reason. Send error.details.resume_token back as the resume query value to keep waiting. A resumed wait still waits for a terminal status or the deadline; it does not return early on progress.

## Properties

Name | Type
------------ | -------------
`data` | object
`meta` | [ResponseMeta](ResponseMeta.md)
`error` | [ProcessingContinuesEnvelopeError](ProcessingContinuesEnvelopeError.md)

## Example

```typescript
import type { ProcessingContinuesEnvelope } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "data": null,
  "meta": null,
  "error": null,
} satisfies ProcessingContinuesEnvelope

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProcessingContinuesEnvelope
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


