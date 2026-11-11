
# SavedJobDefinition

A saved job definition (E4.1-6), kept at a source\'s config.definition and used by a schedule made with source_id. It holds options only: the URL is the source\'s own, and a definition never carries a robots override, a webhook or a ZDR declaration. Dark behind QUARRY_EDGE__SAVED_JOBS (default off). A definition the validators refuse is a 400 with details.fields[] {path, code, message}; every problem found at one level is reported together, under paths that start with definition. The keys url, zdr, webhook and ignore_robots_txt (and the camelCase spellings ignoreRobotsTxt and ignoreRobots) are refused with code not_saveable. The stored form is the canonical one, re-serialised from what the validators accepted. At most 16 KiB of compact JSON (code too_long). Only version, job_kind and options are keys (code unknown_field otherwise).

## Properties

Name | Type
------------ | -------------
`version` | number
`jobKind` | string
`options` | { [key: string]: any; }

## Example

```typescript
import type { SavedJobDefinition } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "version": null,
  "jobKind": null,
  "options": null,
} satisfies SavedJobDefinition

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SavedJobDefinition
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


