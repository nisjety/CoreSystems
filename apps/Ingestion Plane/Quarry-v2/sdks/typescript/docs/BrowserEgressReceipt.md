
# BrowserEgressReceipt


## Properties

Name | Type
------------ | -------------
`sequence` | number
`tabId` | string
`method` | string
`url` | string
`decision` | string
`policy` | string
`timestampMs` | number

## Example

```typescript
import type { BrowserEgressReceipt } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "sequence": null,
  "tabId": null,
  "method": GET,
  "url": https://example.com/path,
  "decision": null,
  "policy": null,
  "timestampMs": null,
} satisfies BrowserEgressReceipt

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserEgressReceipt
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


