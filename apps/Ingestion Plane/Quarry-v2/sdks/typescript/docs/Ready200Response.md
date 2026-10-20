
# Ready200Response


## Properties

Name | Type
------------ | -------------
`status` | string
`durable` | boolean
`browser` | [Ready200ResponseBrowser](Ready200ResponseBrowser.md)
`scrapeBrowser` | [Ready200ResponseScrapeBrowser](Ready200ResponseScrapeBrowser.md)

## Example

```typescript
import type { Ready200Response } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "status": null,
  "durable": null,
  "browser": null,
  "scrapeBrowser": null,
} satisfies Ready200Response

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Ready200Response
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


