
# LiveAgentCursorData

The pointer position of an agent in its attached tab, sampled with the frame. It is sent only on a tabId stream of that tab, while the attachment is live.

## Properties

Name | Type
------------ | -------------
`x` | number
`y` | number
`viewportWidth` | number
`viewportHeight` | number
`sessionRunId` | string
`tabId` | string
`attachId` | string

## Example

```typescript
import type { LiveAgentCursorData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "x": null,
  "y": null,
  "viewportWidth": null,
  "viewportHeight": null,
  "sessionRunId": null,
  "tabId": null,
  "attachId": null,
} satisfies LiveAgentCursorData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as LiveAgentCursorData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


