'use client'

// U6-3 (ui-ux-velion-gap.md §10): permissions editor.
//
// Renders the role list + per-role capability checklist + create/delete
// affordances. Backed by org-core via velion's /api/org/orgs/:id/roles
// catch-all proxy.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Check, Loader2, Plus, Trash2 } from 'lucide-react'

import {
  rbacService,
  type CapabilityCatalogEntry,
  type Role,
} from '@/lib/services/rbac-service'
import { InlineNotice } from '@/components/account/sections/AccountFormPrimitives'

const DEFAULT_ROLE_NAMES = new Set(['owner', 'admin', 'member', 'viewer'])

function groupCatalog(catalog: CapabilityCatalogEntry[]): Map<string, CapabilityCatalogEntry[]> {
  const byGroup = new Map<string, CapabilityCatalogEntry[]>()
  for (const entry of catalog) {
    const list = byGroup.get(entry.group) ?? []
    list.push(entry)
    byGroup.set(entry.group, list)
  }
  return byGroup
}

interface ActiveOrgState {
  loading: boolean
  orgId: string | null
  error: string | null
}

function useActiveOrgId(): ActiveOrgState {
  const [state, setState] = useState<ActiveOrgState>({
    loading: true,
    orgId: null,
    error: null,
  })

  useEffect(() => {
    let cancelled = false
    fetch('/api/user/me/session-context', { credentials: 'include', cache: 'no-store' })
      .then(async (res) => {
        if (!res.ok) {
          throw new Error(`session-context: ${res.status}`)
        }
        const json = (await res.json()) as { organization?: { id?: string } }
        const orgId = json.organization?.id ?? null
        if (!cancelled) setState({ loading: false, orgId, error: null })
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setState({
            loading: false,
            orgId: null,
            error: err instanceof Error ? err.message : 'Failed to resolve active org',
          })
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

  return state
}

interface PermissionsEditorState {
  roles: Role[]
  catalog: CapabilityCatalogEntry[]
  loading: boolean
  error: string | null
}

export function PermissionsEditor() {
  const { loading: orgLoading, orgId, error: orgError } = useActiveOrgId()
  const [state, setState] = useState<PermissionsEditorState>({
    roles: [],
    catalog: [],
    loading: true,
    error: null,
  })

  const refresh = useCallback(async (id: string) => {
    setState((prev) => ({ ...prev, loading: true, error: null }))
    try {
      const [roles, catalog] = await Promise.all([
        rbacService.listRoles(id),
        rbacService.getCapabilityCatalog(id),
      ])
      setState({ roles, catalog, loading: false, error: null })
    } catch (err) {
      setState((prev) => ({
        ...prev,
        loading: false,
        error: err instanceof Error ? err.message : 'Failed to load roles',
      }))
    }
  }, [])

  useEffect(() => {
    if (orgId) {
      void refresh(orgId)
    }
  }, [orgId, refresh])

  if (orgLoading) {
    return (
      <div className="flex items-center gap-2 text-[13px] text-black/60">
        <Loader2 size={14} className="animate-spin" /> Resolving active workspace…
      </div>
    )
  }
  if (orgError || !orgId) {
    return (
      <InlineNotice tone="danger">
        Could not determine the active workspace. {orgError ?? 'Please select an organization and try again.'}
      </InlineNotice>
    )
  }

  if (state.loading) {
    return (
      <div className="flex items-center gap-2 text-[13px] text-black/60">
        <Loader2 size={14} className="animate-spin" /> Loading roles…
      </div>
    )
  }
  if (state.error) {
    return (
      <InlineNotice tone="danger">
        {state.error}
      </InlineNotice>
    )
  }

  return (
    <PermissionsEditorContent
      orgId={orgId}
      roles={state.roles}
      catalog={state.catalog}
      onRefresh={() => void refresh(orgId)}
    />
  )
}

interface PermissionsEditorContentProps {
  orgId: string
  roles: Role[]
  catalog: CapabilityCatalogEntry[]
  onRefresh: () => void
}

function PermissionsEditorContent({
  orgId,
  roles,
  catalog,
  onRefresh,
}: PermissionsEditorContentProps) {
  const groupedCatalog = useMemo(() => groupCatalog(catalog), [catalog])
  const [selectedRoleName, setSelectedRoleName] = useState<string>(
    () => roles[0]?.role_name ?? '',
  )

  // Keep selection valid as roles change.
  useEffect(() => {
    if (!roles.find((r) => r.role_name === selectedRoleName)) {
      setSelectedRoleName(roles[0]?.role_name ?? '')
    }
  }, [roles, selectedRoleName])

  const selectedRole = roles.find((r) => r.role_name === selectedRoleName)

  return (
    <div className="grid gap-6 md:grid-cols-[220px_1fr]">
      {/* ─── Roles sidebar ──────────────────────────────────────────────── */}
      <div>
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-[12px] font-medium uppercase tracking-wide text-[#6B7280]">Roles</h3>
          <NewRoleButton
            orgId={orgId}
            existing={roles.map((r) => r.role_name)}
            onCreated={onRefresh}
          />
        </div>
        <ul className="overflow-hidden rounded-md border border-[#F0F0F0]">
          {roles.map((role) => (
            <li key={role.id}>
              <button
                type="button"
                onClick={() => setSelectedRoleName(role.role_name)}
                className={[
                  'flex w-full items-center justify-between gap-2 border-b border-[#F0F0F0] px-3 py-2 text-left text-[13px] last:border-b-0',
                  role.role_name === selectedRoleName
                    ? 'bg-[#F5F5F5] font-medium text-[#111111]'
                    : 'bg-white text-[#374151] hover:bg-[#FAFAFA]',
                ].join(' ')}
              >
                <span className="truncate">{role.role_name}</span>
                {role.is_custom ? (
                  <span className="rounded-full bg-[#EEF2FF] px-2 py-0.5 text-[10px] font-medium uppercase text-[#4F46E5]">
                    custom
                  </span>
                ) : (
                  <span className="rounded-full bg-[#F3F4F6] px-2 py-0.5 text-[10px] font-medium uppercase text-[#6B7280]">
                    default
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      </div>

      {/* ─── Role detail / editor ───────────────────────────────────────── */}
      <div>
        {selectedRole ? (
          <RoleDetailEditor
            orgId={orgId}
            role={selectedRole}
            groupedCatalog={groupedCatalog}
            onUpdated={onRefresh}
          />
        ) : (
          <p className="text-[13px] text-black/60">Select a role to edit its permissions.</p>
        )}
      </div>
    </div>
  )
}

interface RoleDetailEditorProps {
  orgId: string
  role: Role
  groupedCatalog: Map<string, CapabilityCatalogEntry[]>
  onUpdated: () => void
}

function RoleDetailEditor({
  orgId,
  role,
  groupedCatalog,
  onUpdated,
}: RoleDetailEditorProps) {
  const [selected, setSelected] = useState<Set<string>>(() => new Set(role.permissions))
  const [saving, setSaving] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [savedAt, setSavedAt] = useState<number | null>(null)

  // Reset selection when the active role changes.
  useEffect(() => {
    setSelected(new Set(role.permissions))
    setError(null)
    setSavedAt(null)
  }, [role.id])

  function toggle(key: string) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }

  const isDirty = useMemo(() => {
    if (selected.size !== role.permissions.length) return true
    for (const p of role.permissions) {
      if (!selected.has(p)) return true
    }
    return false
  }, [selected, role.permissions])

  async function save() {
    setSaving(true)
    setError(null)
    try {
      await rbacService.updateRolePermissions(orgId, role.role_name, Array.from(selected))
      setSavedAt(Date.now())
      onUpdated()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save')
    } finally {
      setSaving(false)
    }
  }

  async function remove() {
    if (!confirm(`Delete the "${role.role_name}" role? Members currently assigned to it will need to be re-assigned.`)) {
      return
    }
    setDeleting(true)
    setError(null)
    try {
      await rbacService.deleteRole(orgId, role.role_name)
      onUpdated()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete role')
    } finally {
      setDeleting(false)
    }
  }

  const isDefault = DEFAULT_ROLE_NAMES.has(role.role_name)

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-[15px] font-semibold text-[#111111]">{role.role_name}</h3>
          <p className="mt-0.5 text-[12px] text-[#6B7280]">
            {isDefault
              ? 'Default role — you can adjust permissions but the role itself cannot be deleted.'
              : 'Custom role — fully editable.'}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {role.is_custom && (
            <button
              type="button"
              onClick={remove}
              disabled={deleting}
              className="inline-flex items-center gap-1 rounded-md border border-[#EF4444] px-2.5 py-1.5 text-[12px] font-medium text-[#EF4444] hover:bg-[#FEF2F2] disabled:opacity-50"
            >
              {deleting ? <Loader2 size={12} className="animate-spin" /> : <Trash2 size={12} />}
              Delete
            </button>
          )}
          <button
            type="button"
            onClick={save}
            disabled={!isDirty || saving}
            className="inline-flex items-center gap-1 rounded-md bg-[#111111] px-3 py-1.5 text-[12px] font-medium text-white hover:bg-[#1F2937] disabled:opacity-40"
          >
            {saving && <Loader2 size={12} className="animate-spin" />}
            Save changes
          </button>
        </div>
      </div>

      {error && (
        <div className="mb-3">
          <InlineNotice tone="danger">{error}</InlineNotice>
        </div>
      )}
      {savedAt && !isDirty && !error && (
        <div className="mb-3">
          <InlineNotice tone="success">
            <span className="inline-flex items-center gap-1">
              <Check size={12} /> Permissions saved
            </span>
          </InlineNotice>
        </div>
      )}

      <div className="space-y-4">
        {Array.from(groupedCatalog.entries()).map(([group, entries]) => (
          <fieldset key={group} className="rounded-md border border-[#F0F0F0] p-3">
            <legend className="px-1 text-[11px] font-medium uppercase tracking-wide text-[#6B7280]">{group}</legend>
            <ul className="grid gap-2 sm:grid-cols-2">
              {entries.map((cap) => (
                <li key={cap.key} className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    id={`cap-${role.id}-${cap.key}`}
                    checked={selected.has(cap.key)}
                    onChange={() => toggle(cap.key)}
                    className="mt-0.5 h-4 w-4 rounded border-[#D1D5DB] text-[#111111] focus:ring-[#111111]"
                  />
                  <label
                    htmlFor={`cap-${role.id}-${cap.key}`}
                    className="flex-1 cursor-pointer text-[12px]"
                  >
                    <span className="font-medium text-[#111111]">{cap.label}</span>
                    <span className="ml-1 text-[#9CA3AF]">{cap.key}</span>
                    {cap.description && (
                      <span className="mt-0.5 block text-[11px] text-[#6B7280]">
                        {cap.description}
                      </span>
                    )}
                  </label>
                </li>
              ))}
            </ul>
          </fieldset>
        ))}
      </div>
    </div>
  )
}

interface NewRoleButtonProps {
  orgId: string
  existing: string[]
  onCreated: () => void
}

function NewRoleButton({ orgId, existing, onCreated }: NewRoleButtonProps) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function create() {
    const name = prompt('Name for the new role:')
    if (!name) return
    const trimmed = name.trim()
    if (!trimmed) return
    if (existing.includes(trimmed)) {
      setError(`Role "${trimmed}" already exists.`)
      return
    }
    setBusy(true)
    setError(null)
    try {
      await rbacService.createRole(orgId, { roleName: trimmed, permissions: [] })
      onCreated()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create role')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div>
      <button
        type="button"
        onClick={create}
        disabled={busy}
        className="inline-flex items-center gap-1 rounded-md border border-[#E5E7EB] bg-white px-2 py-1 text-[11px] font-medium text-[#374151] hover:bg-[#F9FAFB] disabled:opacity-50"
      >
        {busy ? <Loader2 size={10} className="animate-spin" /> : <Plus size={10} />}
        New
      </button>
      {error && (
        <p className="mt-1 text-[11px] text-[#EF4444]">{error}</p>
      )}
    </div>
  )
}
