
# AccessibilityNode


## Properties

Name | Type
------------ | -------------
`nodeId` | string
`role` | string
`name` | string
`value` | string
`ignored` | boolean
`frameId` | string
`childIds` | Array&lt;string&gt;

## Example

```typescript
import type { AccessibilityNode } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "nodeId": null,
  "role": null,
  "name": null,
  "value": null,
  "ignored": null,
  "frameId": null,
  "childIds": null,
} satisfies AccessibilityNode

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AccessibilityNode
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


