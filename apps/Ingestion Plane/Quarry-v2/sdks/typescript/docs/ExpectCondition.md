
# ExpectCondition

One checkable condition, tagged by kind. url_matches: `pattern` (1 to 512 bytes; `*` is the only metacharacter). element_present and element_absent: `locator`. text_present and text_absent: `text` (1 to 256 bytes) in the page text or, with `within`, in the one element that `within` names. A locator string is 1 to 256 bytes; a locator of kind nth is refused.

## Properties

Name | Type
------------ | -------------
`kind` | string
`pattern` | string
`locator` | [ExpectLocator](ExpectLocator.md)
`text` | string
`within` | [ExpectLocator](ExpectLocator.md)

## Example

```typescript
import type { ExpectCondition } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "kind": null,
  "pattern": null,
  "locator": null,
  "text": null,
  "within": null,
} satisfies ExpectCondition

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ExpectCondition
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


