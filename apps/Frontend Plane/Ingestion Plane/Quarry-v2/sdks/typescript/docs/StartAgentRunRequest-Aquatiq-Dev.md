
# StartAgentRunRequest


## Properties

Name | Type
------------ | -------------
`constraints` | [AgentConstraints](AgentConstraints.md)
`profileId` | string
`persistProfile` | boolean
`viewport` | [StartAgentRunRequestViewport](StartAgentRunRequestViewport.md)
`zdr` | boolean
`grantId` | string
`resumeRunId` | string
`executionTier` | string
`driverRequirements` | [BrowserDriverCapabilities](BrowserDriverCapabilities.md)

## Example

```typescript
import type { StartAgentRunRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "constraints": null,
  "profileId": null,
  "persistProfile": null,
  "viewport": null,
  "zdr": null,
  "grantId": null,
  "resumeRunId": null,
  "executionTier": null,
  "driverRequirements": null,
} satisfies StartAgentRunRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as StartAgentRunRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


