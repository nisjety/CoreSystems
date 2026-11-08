
# AttachAgentRun200ResponseData


## Properties

Name | Type
------------ | -------------
`attachId` | string
`tabId` | string
`tabEpoch` | number
`controlEpoch` | number
`openedBy` | string
`holder` | string
`allowedDomains` | Array&lt;string&gt;

## Example

```typescript
import type { AttachAgentRun200ResponseData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "attachId": null,
  "tabId": null,
  "tabEpoch": null,
  "controlEpoch": null,
  "openedBy": null,
  "holder": null,
  "allowedDomains": null,
} satisfies AttachAgentRun200ResponseData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AttachAgentRun200ResponseData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


