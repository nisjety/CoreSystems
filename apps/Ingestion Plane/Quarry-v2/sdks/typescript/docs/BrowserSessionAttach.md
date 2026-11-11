
# BrowserSessionAttach

The live agent attachment (WS8.F). Present only while one is live. It carries ids and words only. Gated by QUARRY_EDGE__ATTACH, which is off by default.

## Properties

Name | Type
------------ | -------------
`state` | string
`attachId` | string
`tabId` | string
`tabEpoch` | number
`grantId` | string

## Example

```typescript
import type { BrowserSessionAttach } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "state": null,
  "attachId": null,
  "tabId": null,
  "tabEpoch": null,
  "grantId": null,
} satisfies BrowserSessionAttach

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserSessionAttach
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


