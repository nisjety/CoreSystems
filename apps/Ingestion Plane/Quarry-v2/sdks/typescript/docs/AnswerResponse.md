
# AnswerResponse


## Properties

Name | Type
------------ | -------------
`query` | string
`answer` | string
`citations` | [Array&lt;AnswerCitation&gt;](AnswerCitation.md)
`model` | string
`latencyMs` | number
`sourcesUsed` | number
`sourcesSkipped` | number
`sourcesSnippetFallback` | number

## Example

```typescript
import type { AnswerResponse } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "query": null,
  "answer": null,
  "citations": null,
  "model": null,
  "latencyMs": null,
  "sourcesUsed": null,
  "sourcesSkipped": null,
  "sourcesSnippetFallback": null,
} satisfies AnswerResponse

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AnswerResponse
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


