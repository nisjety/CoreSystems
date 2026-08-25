
# AccessibilityProjection


## Properties

Name | Type
------------ | -------------
`source` | string
`truncated` | boolean
`nodes` | [Array&lt;AccessibilityNode&gt;](AccessibilityNode.md)

## Example

```typescript
import type { AccessibilityProjection } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "source": chromium_cdp_ax,
  "truncated": null,
  "nodes": null,
} satisfies AccessibilityProjection

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AccessibilityProjection
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


