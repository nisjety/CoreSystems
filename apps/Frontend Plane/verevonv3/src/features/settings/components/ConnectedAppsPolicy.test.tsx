// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ConnectedAppsPolicyView } from '@/features/settings/components/ConnectedAppsPolicy'
import {
  connectorPoliciesOf,
  connectorPolicyRequest,
  type ConnectorPolicies,
} from '@/shared/api/orchestration-client'

const defaults: ConnectorPolicies = {
  enforced: true,
  read: 'use_without_asking',
  write: 'always_ask',
  orgRead: 'use_without_asking',
  orgWrite: 'always_ask',
}

afterEach(() => cleanup())

describe('connected-app levels (EP-10 S7)', () => {
  it('reads the person\'s and the org\'s levels from the tool-policy list, and never another capability', () => {
    const levels = connectorPoliciesOf({
      enforced: true,
      policies: [
        { capability_id: 'cap.tool.provider.read', scope_kind: 'user', decision: 'ask' },
        { capability_id: 'cap.tool.provider.execute', scope_kind: 'org', decision: 'never' },
        { capability_id: 'cap.mcp_tool.notes', scope_kind: 'user', decision: 'never' },
      ],
    })
    expect(levels).toEqual({
      enforced: true,
      read: 'ask_first',
      write: 'always_ask',
      orgRead: 'use_without_asking',
      orgWrite: 'off',
    })
  })

  it('reads a list without the enforced bit as not enforced', () => {
    expect(connectorPoliciesOf({ policies: [] }).enforced).toBe(false)
    expect(connectorPoliciesOf([]).enforced).toBe(false)
    expect(connectorPoliciesOf(null)).toEqual({ ...defaults, enforced: false })
  })

  it('writes ask or never on the person\'s own row, and returns to the default by revoking it', () => {
    expect(connectorPolicyRequest({ kind: 'read', level: 'ask_first' })).toEqual({
      capabilityId: 'cap.tool.provider.read',
      method: 'PUT',
      body: { scope: 'user', decision: 'ask', reason: '' },
    })
    expect(connectorPolicyRequest({ kind: 'read', level: 'off' })).toEqual({
      capabilityId: 'cap.tool.provider.read',
      method: 'PUT',
      body: { scope: 'user', decision: 'never', reason: '' },
    })
    expect(connectorPolicyRequest({ kind: 'write', level: 'off' })).toEqual({
      capabilityId: 'cap.tool.provider.execute',
      method: 'PUT',
      body: { scope: 'user', decision: 'never', reason: '' },
    })
    expect(connectorPolicyRequest({ kind: 'read', level: 'use_without_asking' })).toEqual({
      capabilityId: 'cap.tool.provider.read',
      method: 'DELETE',
    })
    expect(connectorPolicyRequest({ kind: 'write', level: 'always_ask' })).toEqual({
      capabilityId: 'cap.tool.provider.execute',
      method: 'DELETE',
    })
  })

  it('says Not enforced yet while the deployment does not enforce the levels', () => {
    render(() => <ConnectedAppsPolicyView policies={{ ...defaults, enforced: false }} busy={false} onChange={vi.fn()} />)
    flush()
    expect(screen.getByText(/ikke håndhevet ennå|not enforced yet/i)).toBeTruthy()
  })

  it('does not say Not enforced yet once the levels are enforced', () => {
    render(() => <ConnectedAppsPolicyView policies={defaults} busy={false} onChange={vi.fn()} />)
    flush()
    expect(screen.queryByText(/ikke håndhevet ennå|not enforced yet/i)).toBeNull()
  })

  it('offers only tightening levels: three for reads, and always ask or off for writes', () => {
    render(() => <ConnectedAppsPolicyView policies={defaults} busy={false} onChange={vi.fn()} />)
    flush()
    const read = document.getElementById('connected-apps-policy-read') as HTMLSelectElement
    const write = document.getElementById('connected-apps-policy-write') as HTMLSelectElement
    expect(Array.from(read.options).map((option) => option.value)).toEqual(['use_without_asking', 'ask_first', 'off'])
    expect(Array.from(write.options).map((option) => option.value)).toEqual(['always_ask', 'off'])
  })

  it('hands a changed level to the parent and ignores the level already set', () => {
    const onChange = vi.fn()
    render(() => <ConnectedAppsPolicyView policies={defaults} busy={false} onChange={onChange} />)
    flush()
    const read = document.getElementById('connected-apps-policy-read') as HTMLSelectElement
    fireEvent.change(read, { target: { value: 'off' } })
    flush()
    expect(onChange).toHaveBeenCalledWith({ kind: 'read', level: 'off' })

    onChange.mockClear()
    const write = document.getElementById('connected-apps-policy-write') as HTMLSelectElement
    fireEvent.change(write, { target: { value: 'always_ask' } })
    flush()
    expect(onChange).not.toHaveBeenCalled()
  })

  it('says when the org\'s stricter level is the one in force', () => {
    render(() => (
      <ConnectedAppsPolicyView policies={{ ...defaults, orgRead: 'off', orgWrite: 'off' }} busy={false} onChange={vi.fn()} />
    ))
    flush()
    expect(screen.getAllByText(/det strengeste valget gjelder|the stricter choice applies/i)).toHaveLength(2)
  })

  it('tells the person that Ask first leaves connected apps out of Ask mode', () => {
    render(() => <ConnectedAppsPolicyView policies={{ ...defaults, read: 'ask_first' }} busy={false} onChange={vi.fn()} />)
    flush()
    expect(screen.getByText(/utfør-modus|do mode/i)).toBeTruthy()
  })
})
