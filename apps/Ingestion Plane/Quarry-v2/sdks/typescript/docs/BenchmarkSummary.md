
# BenchmarkSummary


## Properties

Name | Type
------------ | -------------
`benchmarkId` | string
`orgId` | string
`name` | string
`suite` | string
`baseline` | string
`status` | string
`lastRunAt` | Date
`latestScore` | number

## Example

```typescript
import type { BenchmarkSummary } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "benchmarkId": null,
  "orgId": null,
  "name": null,
  "suite": null,
  "baseline": null,
  "status": null,
  "lastRunAt": null,
  "latestScore": null,
} satisfies BenchmarkSummary

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BenchmarkSummary
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


