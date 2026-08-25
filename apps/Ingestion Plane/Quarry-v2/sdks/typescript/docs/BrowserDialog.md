
# BrowserDialog


## Properties

Name | Type
------------ | -------------
`dialogId` | string
`frameId` | string
`kind` | string
`message` | string
`defaultPrompt` | string
`origin` | string
`openedAtMs` | number

## Example

```typescript
import type { BrowserDialog } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "dialogId": null,
  "frameId": null,
  "kind": null,
  "message": null,
  "defaultPrompt": null,
  "origin": null,
  "openedAtMs": null,
} satisfies BrowserDialog

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BrowserDialog
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


