
# AttachAgentCurrentTabRequest


## Properties

Name | Type
------------ | -------------
`grantId` | string
`attachTab` | [AttachAgentCurrentTabRequestAttachTab](AttachAgentCurrentTabRequestAttachTab.md)
`constraints` | [AttachAgentRunRequestConstraints](AttachAgentRunRequestConstraints.md)
`policy` | [RequestAsk](RequestAsk.md)

## Example

```typescript
import type { AttachAgentCurrentTabRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "grantId": null,
  "attachTab": null,
  "constraints": null,
  "policy": null,
} satisfies AttachAgentCurrentTabRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AttachAgentCurrentTabRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


