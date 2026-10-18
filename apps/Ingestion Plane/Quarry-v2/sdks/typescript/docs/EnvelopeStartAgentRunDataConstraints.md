
# EnvelopeStartAgentRunDataConstraints

The bounds the run runs under after defaults and ceilings. Present when QUARRY_EDGE__BUDGET_ENFORCEMENT is not off. max_* are absent for an interactive (human) run, which is never capped. Cost is estimated from a flat action table.

## Properties

Name | Type
------------ | -------------
`_class` | string
`enforcement` | string
`maxSteps` | number
`maxRuntimeS` | number
`maxCostUsd` | number
`estimated` | boolean

## Example

```typescript
import type { EnvelopeStartAgentRunDataConstraints } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "_class": null,
  "enforcement": null,
  "maxSteps": null,
  "maxRuntimeS": null,
  "maxCostUsd": null,
  "estimated": null,
} satisfies EnvelopeStartAgentRunDataConstraints

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as EnvelopeStartAgentRunDataConstraints
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


