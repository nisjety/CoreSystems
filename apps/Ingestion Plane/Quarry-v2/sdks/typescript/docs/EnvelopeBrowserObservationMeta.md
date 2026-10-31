
# EnvelopeBrowserObservationMeta

Present on a step response of an agent-class run (WS6.B); absent for a person\'s session and when budgets are off.

## Properties

Name | Type
------------ | -------------
`budget` | [BudgetMeta](BudgetMeta.md)

## Example

```typescript
import type { EnvelopeBrowserObservationMeta } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "budget": null,
} satisfies EnvelopeBrowserObservationMeta

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as EnvelopeBrowserObservationMeta
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


