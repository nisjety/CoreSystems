
# LiveViewRef

The run\'s live-view reference (LiveViewRef): a URL that the App Shell renders, its kind, and when it expires. Its keys are snake_case. It is the same object as the live_view event of the frame stream and the data of GET .../live-view.

## Properties

Name | Type
------------ | -------------
`url` | string
`kind` | string
`expiresAt` | Date

## Example

```typescript
import type { LiveViewRef } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "kind": null,
  "expiresAt": null,
} satisfies LiveViewRef

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as LiveViewRef
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


