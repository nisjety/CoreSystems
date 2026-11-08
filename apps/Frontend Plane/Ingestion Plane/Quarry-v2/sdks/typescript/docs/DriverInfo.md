
# DriverInfo


## Properties

Name | Type
------------ | -------------
`kind` | string
`durationMs` | number
`profile` | string
`version` | string
`sessionId` | string
`liveViewUrl` | string
`recordingId` | string

## Example

```typescript
import type { DriverInfo } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "kind": null,
  "durationMs": null,
  "profile": null,
  "version": null,
  "sessionId": null,
  "liveViewUrl": null,
  "recordingId": null,
} satisfies DriverInfo

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as DriverInfo
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


