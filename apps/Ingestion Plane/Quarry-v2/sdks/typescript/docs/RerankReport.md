
# RerankReport

What the rerank step did for this search. The field is absent when no rerank step is wired in. applied: the Data Plane\'s scores ordered the head of the results. skipped: the Data Plane was not asked, and the reason says why (disabled, misconfigured, processor_unapproved, metering_unavailable, zdr, sovereign, circuit_open, input_not_tenant, input_too_small, empty_query, or unknown for a Data Plane reason Quarry does not name). degraded: it was asked and failed, so the results keep their input order, and the reason says how (refused, contract, rate_limited, provider_rate_limited, provider_error, unreachable, token_unavailable, timeout, no_scores). shadow: it was asked and its order was measured, but the results keep their order and carry no score. Only applied and rerank-not-configured outcomes are cached.

## Properties

Name | Type
------------ | -------------
`status` | string
`reason` | string
`profile` | string

## Example

```typescript
import type { RerankReport } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "status": null,
  "reason": null,
  "profile": null,
} satisfies RerankReport

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as RerankReport
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


