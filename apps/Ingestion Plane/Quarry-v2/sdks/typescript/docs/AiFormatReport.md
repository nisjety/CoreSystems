
# AiFormatReport

What one AI format did. A model failure is a status in a 200 body, never an HTTP error: ok (a result field is present), error (a model call was made and failed) or skipped (no model call was made for this format). These codes are strings in the body, not wire error codes.

## Properties

Name | Type
------------ | -------------
`status` | string
`code` | string
`reason` | string
`dimension` | string
`retryAfterS` | number
`model` | string
`usage` | [AiFormatReportUsage](AiFormatReportUsage.md)
`truncatedInput` | boolean
`untrustedSource` | boolean
`schemaValid` | boolean
`repair` | string
`schemaIssues` | [Array&lt;AiFormatReportSchemaIssuesInner&gt;](AiFormatReportSchemaIssuesInner.md)
`prefilled` | [Array&lt;AiFormatReportPrefilledInner&gt;](AiFormatReportPrefilledInner.md)
`found` | boolean
`grounded` | boolean
`dropped` | number

## Example

```typescript
import type { AiFormatReport } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "status": null,
  "code": null,
  "reason": null,
  "dimension": null,
  "retryAfterS": null,
  "model": null,
  "usage": null,
  "truncatedInput": null,
  "untrustedSource": null,
  "schemaValid": null,
  "repair": null,
  "schemaIssues": null,
  "prefilled": null,
  "found": null,
  "grounded": null,
  "dropped": null,
} satisfies AiFormatReport

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AiFormatReport
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


