
# NotifyOnChangeFilter

The change types a subscription takes, and the paragraph threshold for a changed page. A subscription without one takes every change. It filters `change_detected` events only.

## Properties

Name | Type
------------ | -------------
`types` | Array&lt;string&gt;
`minParagraphDelta` | number

## Example

```typescript
import type { NotifyOnChangeFilter } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "types": null,
  "minParagraphDelta": null,
} satisfies NotifyOnChangeFilter

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as NotifyOnChangeFilter
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


