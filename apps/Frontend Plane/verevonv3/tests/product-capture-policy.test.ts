import { describe, expect, it } from 'vitest'
import { resolve } from 'node:path'
import { productCaptureSettings } from './e2e/product-capture-policy'

const root = resolve('fixture-repository')
const output = resolve('private-recording-evidence')
const ready = () => ({
  recordingReadiness: { status: 'ready-for-recording', gates: Object.fromEntries(['correctness','repeatability','durability','recovery','trust','experience','performance'].map(gate => [gate, 'passed'])) },
  tasks: ['01-kundesvar','02-salgsrapport','03-kampanje','04-prosjektplan'].map(id => ({ id, currentEvidence: { consecutivePassesOnReleaseBuild: 5 } })),
})

describe('private recording and release capture', () => {
  it('leaves ordinary acceptance tests unchanged', () => expect(productCaptureSettings(undefined, { tasks: [] }, undefined, root)).toBeUndefined())
  it('allows private rehearsal without approving publication', () => {
    expect(productCaptureSettings('rehearsal', { tasks: [] }, output, root)).toEqual({ mode: 'rehearsal', outputDir: output, publicationApproved: false })
  })
  it.each([root, resolve(root,'public','recording'), resolve(root,'../..'), 'relative-evidence'])('rejects an unsafe output: %s', path => {
    expect(() => productCaptureSettings('rehearsal', ready(), path, root)).toThrow(/outside|absolute/)
  })
  it('rejects unknown modes instead of silently disabling the gate', () => expect(() => productCaptureSettings('preview', ready(), output, root)).toThrow('MODE'))
  it('keeps a blocked pack blocked in release mode', () => expect(() => productCaptureSettings('release', { tasks: [] }, output, root)).toThrow('gated'))
  it.each(['correctness','repeatability','durability','recovery','trust','experience','performance'])('requires the %s gate', gate => {
    const pack = ready(); pack.recordingReadiness.gates[gate] = 'pending'
    expect(() => productCaptureSettings('release', pack, output, root)).toThrow('gated')
  })
  it.each([0,4,5.5,NaN,Infinity])('rejects an invalid pass count: %s', count => {
    const pack = ready(); pack.tasks[0].currentEvidence.consecutivePassesOnReleaseBuild = count
    expect(() => productCaptureSettings('release', pack, output, root)).toThrow('gated')
  })
  it('rejects duplicate scenarios even when there are four rows', () => {
    const pack = ready(); pack.tasks[3].id = pack.tasks[0].id
    expect(() => productCaptureSettings('release', pack, output, root)).toThrow('gated')
  })
  it('allows a qualified capture without turning it into publishing permission', () => {
    expect(productCaptureSettings('release', ready(), output, root)?.publicationApproved).toBe(false)
  })
})
