
# ScrapeRequestFormatsInner


## Properties

Name | Type
------------ | -------------
`type` | string
`schema` | object
`prompt` | string
`maxChars` | number
`language` | string
`question` | string
`query` | string
`maxHighlights` | number
`maxCharsEach` | number

## Example

```typescript
import type { ScrapeRequestFormatsInner } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "type": null,
  "schema": null,
  "prompt": null,
  "maxChars": null,
  "language": null,
  "question": null,
  "query": null,
  "maxHighlights": null,
  "maxCharsEach": null,
} satisfies ScrapeRequestFormatsInner

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScrapeRequestFormatsInner
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


