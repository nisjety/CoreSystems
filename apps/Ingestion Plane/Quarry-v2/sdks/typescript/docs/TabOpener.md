
# TabOpener

Who opened a tab (`openedBy` on a tab of the owner projection).

## Properties

Name | Type
------------ | -------------
`initiatedBy` | string
`agent` | [AgentIdentity](AgentIdentity.md)

## Example

```typescript
import type { TabOpener } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "initiatedBy": null,
  "agent": null,
} satisfies TabOpener

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as TabOpener
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


