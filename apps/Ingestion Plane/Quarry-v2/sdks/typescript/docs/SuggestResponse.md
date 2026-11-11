
# SuggestResponse

The suggestions for one query. related_queries is always present, and may be empty. corrected_query is present only when the query likely has a typo, and entity only when the query names one specific entity.

## Properties

Name | Type
------------ | -------------
`correctedQuery` | string
`relatedQueries` | Array&lt;string&gt;
`entity` | [SuggestEntityPanel](SuggestEntityPanel.md)

## Example

```typescript
import type { SuggestResponse } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "correctedQuery": null,
  "relatedQueries": null,
  "entity": null,
} satisfies SuggestResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SuggestResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


