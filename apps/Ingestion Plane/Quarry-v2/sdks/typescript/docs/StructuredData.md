
# StructuredData

Machine-readable facts harvested from the page\'s own markup — hydration payloads, JSON-LD, microdata and tables — alongside the prose. Present only when the page carried any, and independent of `status`: a schema-guided extraction and a markdown one read the same page. Added because readability strips `<script>` by design, which on a hydration-rendered page (Enonic, Next.js, Nuxt) discards the very figures the page exists to publish.

## Properties

Name | Type
------------ | -------------
`figures` | [Array&lt;StructuredKeyFigure&gt;](StructuredKeyFigure.md)
`tables` | Array&lt;object&gt;
`microdata` | Array&lt;object&gt;
`blobs` | Array&lt;object&gt;
`truncated` | boolean

## Example

```typescript
import type { StructuredData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "figures": null,
  "tables": null,
  "microdata": null,
  "blobs": null,
  "truncated": null,
} satisfies StructuredData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as StructuredData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


