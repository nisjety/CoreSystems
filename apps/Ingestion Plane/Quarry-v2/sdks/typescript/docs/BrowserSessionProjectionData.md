
# BrowserSessionProjectionData

The owner projection of a run (BrowserSessionProjectionData). A live run and a released run share this shape, and `live` says which one it is. Keys are camelCase. A field that is absent, rather than false or null, is marked so.

## Properties

Name | Type
------------ | -------------
`runId` | string
`leaseId` | string
`profileId` | string
`profileStorage` | string
`profileScope` | string
`viewport` | [BrowserSessionProjectionDataViewport](BrowserSessionProjectionDataViewport.md)
`currentUrl` | string
`step` | number
`status` | string
`live` | boolean
`zdr` | boolean
`controlMode` | string
`controlEpoch` | number
`executionTier` | string
`tabs` | [Array&lt;BrowserSessionTab&gt;](BrowserSessionTab.md)
`lastObservation` | [BrowserObservation](BrowserObservation.md)
`grantId` | string
`profileRestore` | [ProfileRestoreSummary](ProfileRestoreSummary.md)
`grantKind` | string
`tabControl` | boolean
`attachAvailable` | boolean
`attach` | [BrowserSessionAttach](BrowserSessionAttach.md)
`runOrigin` | string

## Example

```typescript
import type { BrowserSessionProjectionData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "runId": null,
  "leaseId": null,
  "profileId": null,
  "profileStorage": null,
  "profileScope": null,
  "viewport": null,
  "currentUrl": null,
  "step": null,
  "status": null,
  "live": null,
  "zdr": null,
  "controlMode": null,
  "controlEpoch": null,
  "executionTier": null,
  "tabs": null,
  "lastObservation": null,
  "grantId": null,
  "profileRestore": null,
  "grantKind": null,
  "tabControl": null,
  "attachAvailable": null,
  "attach": null,
  "runOrigin": null,
} satisfies BrowserSessionProjectionData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserSessionProjectionData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


