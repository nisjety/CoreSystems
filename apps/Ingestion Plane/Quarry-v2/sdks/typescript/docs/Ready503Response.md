
# Ready503Response


## Properties

Name | Type
------------ | -------------
`status` | string
`durable` | boolean
`reason` | string
`browser` | [Ready503ResponseBrowser](Ready503ResponseBrowser.md)
`scrapeBrowser` | [Ready200ResponseScrapeBrowser](Ready200ResponseScrapeBrowser.md)

## Example

```typescript
import type { Ready503Response } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "status": null,
  "durable": null,
  "reason": null,
  "browser": null,
  "scrapeBrowser": null,
} satisfies Ready503Response

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as Ready503Response
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


