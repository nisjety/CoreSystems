
# OutputFormats


## Properties

Name | Type
------------ | -------------
`html` | [FormatRef](FormatRef.md)
`markdown` | [FormatRef](FormatRef.md)
`raw` | [FormatRef](FormatRef.md)
`links` | [Array&lt;Link&gt;](Link.md)
`screenshot` | [FormatRef](FormatRef.md)
`pdf` | [FormatRef](FormatRef.md)
`extract` | [ExtractRef](ExtractRef.md)

## Example

```typescript
import type { OutputFormats } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "html": null,
  "markdown": null,
  "raw": null,
  "links": null,
  "screenshot": null,
  "pdf": null,
  "extract": null,
} satisfies OutputFormats

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as OutputFormats
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


