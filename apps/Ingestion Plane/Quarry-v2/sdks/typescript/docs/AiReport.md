
# AiReport

data.ai: one entry per AI format asked for, and a total. Token usage is reported; nothing is billed for it in this version.

## Properties

Name | Type
------------ | -------------
`json` | [AiFormatReport](AiFormatReport.md)
`summary` | [AiFormatReport](AiFormatReport.md)
`question` | [AiFormatReport](AiFormatReport.md)
`highlights` | [AiFormatReport](AiFormatReport.md)
`total` | [AiReportTotal](AiReportTotal.md)

## Example

```typescript
import type { AiReport } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "json": null,
  "summary": null,
  "question": null,
  "highlights": null,
  "total": null,
} satisfies AiReport

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AiReport
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


