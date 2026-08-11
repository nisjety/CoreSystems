
# AgentConstraints


## Properties

Name | Type
------------ | -------------
`maxSteps` | number
`allowedDomains` | Array&lt;string&gt;
`maxRuntimeS` | number
`maxCostUsd` | number

## Example

```typescript
import type { AgentConstraints } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "maxSteps": null,
  "allowedDomains": null,
  "maxRuntimeS": null,
  "maxCostUsd": null,
} satisfies AgentConstraints

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AgentConstraints
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


