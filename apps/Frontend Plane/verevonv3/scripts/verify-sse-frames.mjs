#!/usr/bin/env node
// Checks a captured SSE transcript (`curl -N` of one chat turn) against the Rust-generated payload
// golden (src/shared/api/contracts/sse-event-payloads.json, vendored from
// apps/Model Plane/api/contracts, WS12 S7). Every frame whose event name the golden knows must carry
// every key the golden marks `always`, no key it does not list, and a JSON type it allows for that key.
//
//   node scripts/verify-sse-frames.mjs <transcript> [--golden <path>] [--strict]
//
// Events the golden does not describe (transport frames such as `connected`, `chunk` and `done`, and
// the orchestration events) are counted and listed, and fail only under --strict. Exit status is 1 when
// a frame violates the golden, 2 when the transcript or golden cannot be read.
//
// The transcript is read as data and never echoed beyond event names, keys and types, so a ZDR turn's
// capture can be checked without its content reaching a log.
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const DEFAULT_GOLDEN = resolve(here, '../src/shared/api/contracts/sse-event-payloads.json')

function parseArgs(argv) {
  const options = { transcript: null, golden: DEFAULT_GOLDEN, strict: false }
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    if (arg === '--strict') options.strict = true
    else if (arg === '--golden') options.golden = resolve(argv[(i += 1)] ?? '')
    else if (!options.transcript) options.transcript = arg
    else throw new Error(`unexpected argument: ${arg}`)
  }
  if (!options.transcript) throw new Error('usage: verify-sse-frames.mjs <transcript> [--golden <path>] [--strict]')
  return options
}

/** SSE frames of a transcript: blank-line separated blocks of `field: value` lines. */
export function parseFrames(text) {
  const frames = []
  for (const block of text.replace(/\r\n/g, '\n').split(/\n\n+/)) {
    let event = 'message'
    let id = null
    const data = []
    for (const line of block.split('\n')) {
      if (!line || line.startsWith(':')) continue
      const colon = line.indexOf(':')
      const field = colon === -1 ? line : line.slice(0, colon)
      const value = colon === -1 ? '' : line.slice(colon + 1).replace(/^ /, '')
      if (field === 'event') event = value
      else if (field === 'id') id = value
      else if (field === 'data') data.push(value)
    }
    if (data.length || event !== 'message') frames.push({ event, id, data: data.join('\n') })
  }
  return frames
}

function jsonType(value) {
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'array'
  if (typeof value === 'number') return Number.isInteger(value) ? 'integer' : 'number'
  return typeof value
}

/** An integer is a valid `number` (a float such as 0.0 serialises without a fraction), not the reverse. */
function typeAllowed(types, actual) {
  return types.includes(actual) || (actual === 'integer' && types.includes('number'))
}

/** The violations of one frame against its golden entry; an empty list means the frame conforms. */
export function checkFrame(frame, shape) {
  let payload
  try {
    payload = JSON.parse(frame.data)
  } catch {
    return ['data is not JSON']
  }
  if (payload === null || typeof payload !== 'object' || Array.isArray(payload)) return ['data is not a JSON object']
  const problems = []
  for (const [key, spec] of Object.entries(shape.keys)) {
    if (spec.presence === 'always' && !(key in payload)) problems.push(`missing key "${key}"`)
  }
  for (const [key, value] of Object.entries(payload)) {
    const spec = shape.keys[key]
    if (!spec) problems.push(`unexpected key "${key}"`)
    else if (!typeAllowed(spec.types, jsonType(value))) {
      problems.push(`key "${key}" is ${jsonType(value)}, the golden allows ${spec.types.join('|')}`)
    }
  }
  return problems
}

function main() {
  let options
  let golden
  let transcript
  try {
    options = parseArgs(process.argv.slice(2))
    golden = JSON.parse(readFileSync(options.golden, 'utf8'))
    transcript = readFileSync(options.transcript, 'utf8')
  } catch (error) {
    console.error(`verify-sse-frames: ${error.message}`)
    return 2
  }
  const frames = parseFrames(transcript)
  const checked = new Map()
  const outside = new Map()
  let violations = 0
  frames.forEach((frame, index) => {
    const shape = golden.events[frame.event]
    if (!shape) {
      outside.set(frame.event, (outside.get(frame.event) ?? 0) + 1)
      return
    }
    checked.set(frame.event, (checked.get(frame.event) ?? 0) + 1)
    for (const problem of checkFrame(frame, shape)) {
      violations += 1
      console.error(`not ok - frame ${index + 1} (${frame.event}): ${problem}`)
    }
  })
  for (const [event, count] of checked) console.log(`ok - ${event}: ${count} frame(s) checked`)
  for (const [event, count] of outside) console.log(`skip - ${event}: ${count} frame(s) not described by the golden`)
  const unseen = Object.keys(golden.events).filter((event) => !checked.has(event))
  console.log(`not seen in this transcript: ${unseen.length ? unseen.join(', ') : 'none'}`)
  if (!frames.length) {
    console.error('not ok - the transcript holds no SSE frames')
    return 1
  }
  if (options.strict && outside.size) {
    console.error(`not ok - --strict: events outside the golden: ${[...outside.keys()].join(', ')}`)
    return 1
  }
  console.log(`${violations === 0 ? 'ok' : 'not ok'} - ${frames.length} frame(s), ${violations} violation(s)`)
  return violations === 0 ? 0 : 1
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
