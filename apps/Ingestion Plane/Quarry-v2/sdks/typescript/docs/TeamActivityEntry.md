
# TeamActivityEntry


## Properties

Name | Type
------------ | -------------
`eventId` | string
`orgId` | string
`eventType` | string
`runId` | string
`ts` | Date
`summary` | string

## Example

```typescript
import type { TeamActivityEntry } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "eventId": null,
  "orgId": null,
  "eventType": null,
  "runId": null,
  "ts": null,
  "summary": null,
} satisfies TeamActivityEntry

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as TeamActivityEntry
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


