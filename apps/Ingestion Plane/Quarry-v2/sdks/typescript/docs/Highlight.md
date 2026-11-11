
# Highlight

An exact span of the page text. start and end are Unicode scalar (char) offsets, end-exclusive, into the AI source text; text is the slice (8 to max_chars_each characters).

## Properties

Name | Type
------------ | -------------
`text` | string
`start` | number
`end` | number

## Example

```typescript
import type { Highlight } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "text": null,
  "start": null,
  "end": null,
} satisfies Highlight

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Highlight
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


