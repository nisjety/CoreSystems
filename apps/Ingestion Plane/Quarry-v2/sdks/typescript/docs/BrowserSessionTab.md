
# BrowserSessionTab

One tab of the owner projection.

## Properties

Name | Type
------------ | -------------
`tabId` | string
`title` | string
`url` | string
`active` | boolean
`tabEpoch` | number
`holder` | string
`openedBy` | [TabOpener](TabOpener.md)
`mark` | string
`attachId` | string

## Example

```typescript
import type { BrowserSessionTab } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "tabId": null,
  "title": null,
  "url": null,
  "active": null,
  "tabEpoch": null,
  "holder": null,
  "openedBy": null,
  "mark": null,
  "attachId": null,
} satisfies BrowserSessionTab

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserSessionTab
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


