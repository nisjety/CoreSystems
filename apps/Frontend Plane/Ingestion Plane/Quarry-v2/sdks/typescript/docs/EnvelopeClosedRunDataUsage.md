
# EnvelopeClosedRunDataUsage

An agent-class run\'s spend, as it stood when the run ended (WS6.B). Absent for a person\'s session, which has no budget.

## Properties

Name | Type
------------ | -------------
`_class` | string
`spentUsd` | number
`steps` | number
`attempts` | number
`activeS` | number
`exhausted` | string

## Example

```typescript
import type { EnvelopeClosedRunDataUsage } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "_class": null,
  "spentUsd": null,
  "steps": null,
  "attempts": null,
  "activeS": null,
  "exhausted": null,
} satisfies EnvelopeClosedRunDataUsage

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as EnvelopeClosedRunDataUsage
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


