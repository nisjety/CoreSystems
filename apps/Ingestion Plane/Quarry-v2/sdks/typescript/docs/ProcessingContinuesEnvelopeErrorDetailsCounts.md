
# ProcessingContinuesEnvelopeErrorDetailsCounts

Completed, failed, blocked and retried counts when the wait ran out.

## Properties

Name | Type
------------ | -------------
`completed` | number
`failed` | number
`blocked` | number
`retried` | number

## Example

```typescript
import type { ProcessingContinuesEnvelopeErrorDetailsCounts } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "completed": null,
  "failed": null,
  "blocked": null,
  "retried": null,
} satisfies ProcessingContinuesEnvelopeErrorDetailsCounts

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProcessingContinuesEnvelopeErrorDetailsCounts
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


