
# AgentIdentity

Which agent was behind an operation an agent initiated. Ids and classes only. Only `principalId` of a `service_principal` basis is verified (it is the calling token\'s signed service id); `aiRunId` and `label` are what the caller said, advisory, and never used to decide anything.

## Properties

Name | Type
------------ | -------------
`basis` | string
`principalId` | string
`aiRunId` | string
`label` | string

## Example

```typescript
import type { AgentIdentity } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "basis": null,
  "principalId": null,
  "aiRunId": null,
  "label": null,
} satisfies AgentIdentity

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AgentIdentity
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


