
# StepExpectation

E4.2 step 1: a postcondition the page must meet once this step ran. Quarry checks it against the observation it reads, never the Model\'s word. It holds when every condition in `all` holds. Unknown fields are refused. Dark: a step that carries one is refused with 400 `unsupported_option` unless QUARRY_EDGE__STEP_EXPECT is on (default off). A ZDR run may send it; the verdict is returned on the observation (proof_bundle.expectation) and nothing is stored. A bounds violation is a 400 BAD_REQUEST whose details.field names the path of the refused field, never its value.

## Properties

Name | Type
------------ | -------------
`all` | [Array&lt;ExpectCondition&gt;](ExpectCondition.md)

## Example

```typescript
import type { StepExpectation } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "all": null,
} satisfies StepExpectation

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as StepExpectation
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


