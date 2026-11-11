
# ScrapeAiFormat

An AI format of a scrape (INNH-1). Unknown keys in the object are 400 unknown_field. Every field error is 400 with details.fields[] {path, code, message} and never echoes a value. A json schema may use only the keywords type, properties, required, items, enum, const, anyOf, oneOf, minItems, maxItems, minLength, maxLength, minimum, maximum, additionalProperties (true or false), description and title ($schema, $id and $comment are dropped). A local $ref to #/$defs/ or #/definitions/ is inlined in its place (those two root keywords are dropped), and only title and description may sit beside it. A definition that refers back to itself is 400 schema_recursive; any other $ref is 400 schema_ref_not_supported, and a $ref that names no definition is 400 invalid_value, all at the $ref. Any other keyword (for example allOf, not, pattern, format) is 400 schema_keyword_not_supported at its path, because the validator would ignore it. The schema root must be an object schema once the references are inlined; at most 65,536 bytes, 10 levels and 200 properties, counted on the inlined schema (schema_root_not_object, schema_too_large, schema_too_complex).

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
import type { ScrapeAiFormat } from '@quarry/client'

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
} satisfies ScrapeAiFormat

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScrapeAiFormat
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


