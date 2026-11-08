
# OpenAgentTabRequest


## Properties

Name | Type
------------ | -------------
`url` | string
`initiatedBy` | string
`aiRunId` | string
`agentLabel` | string

## Example

```typescript
import type { OpenAgentTabRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "initiatedBy": null,
  "aiRunId": null,
  "agentLabel": null,
} satisfies OpenAgentTabRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as OpenAgentTabRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


