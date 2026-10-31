
# BudgetMeta

The budget ledger of an agent-class run after a step (WS6.B). Cost is estimated from the flat action table plus a browser-time term, never billed usage. Enforcement is QUARRY_EDGE__BUDGET_ENFORCEMENT (shadow by default: the bounds are reported and what would be refused is recorded, nothing is refused).

## Properties

Name | Type
------------ | -------------
`_class` | string
`steps` | number
`attempts` | number
`maxSteps` | number
`maxAttempts` | number
`elapsedS` | number
`maxRuntimeS` | number
`spentUsd` | number
`maxCostUsd` | number
`estimated` | boolean
`costSource` | string
`exhausted` | string

## Example

```typescript
import type { BudgetMeta } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "_class": null,
  "steps": null,
  "attempts": null,
  "maxSteps": null,
  "maxAttempts": null,
  "elapsedS": null,
  "maxRuntimeS": null,
  "spentUsd": null,
  "maxCostUsd": null,
  "estimated": null,
  "costSource": null,
  "exhausted": null,
} satisfies BudgetMeta

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BudgetMeta
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


