
# AttachAgentRunRequest


## Properties

Name | Type
------------ | -------------
`grantId` | string
`startUrl` | string
`constraints` | [AttachAgentRunRequestConstraints](AttachAgentRunRequestConstraints.md)

## Example

```typescript
import type { AttachAgentRunRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "grantId": null,
  "startUrl": null,
  "constraints": null,
} satisfies AttachAgentRunRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AttachAgentRunRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


