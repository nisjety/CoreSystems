'use client'

import { useState, useSyncExternalStore } from 'react'
import { Loader2, Check, AlertCircle, Building2 } from 'lucide-react'
import { useCurrentOrganization } from '@/components/core/profile/hooks/useProfile'
import type { WorkspaceGeneralFormValues } from '../types'

function Label({ children }: { children: React.ReactNode }) {
  return (
    <label className="mb-1.5 block font-inter text-[11px] font-medium uppercase tracking-widest text-[#9B9691]">
      {children}
    </label>
  )
}

function Field({
  value,
  onChange,
  placeholder,
  readOnly,
  hint,
}: {
  value: string
  onChange?: (v: string) => void
  placeholder?: string
  readOnly?: boolean
  hint?: string
}) {
  return (
    <div>
      <input
        value={value}
        readOnly={readOnly}
        onChange={e => onChange?.(e.target.value)}
        placeholder={placeholder}
        className={[
          'block w-full rounded-xl border border-[#D8D2C6] bg-white px-3.5 py-2.5',
          'font-inter text-[13px] text-[#1C1C1A] placeholder:text-[#C8C1B3]',
          'outline-none transition-colors focus:border-[#1C1C1A] focus:ring-1 focus:ring-[#1C1C1A]/10',
          readOnly ? 'cursor-default opacity-60' : '',
        ].join(' ')}
      />
      {hint && (
        <p className="mt-1 font-inter text-[11px] text-[#9B9691]">{hint}</p>
      )}
    </div>
  )
}

export function GeneralSection() {
  const { data: org, isLoading } = useCurrentOrganization()
  const hydrated = useSyncExternalStore(
    () => () => {},
    () => true,
    () => false,
  )

  const [form, setForm] = useState<WorkspaceGeneralFormValues>({
    name:        org?.name ?? '',
    slug:        org?.slug ?? '',
    description: '',
    website:     '',
  })
  const [saved, setSaved] = useState(false)

  const set = (k: keyof WorkspaceGeneralFormValues) => (v: string) =>
    setForm(prev => ({ ...prev, [k]: v }))

  const handleSave = async () => {
    // TODO: wire to org update mutation when backend supports it
    setSaved(true)
    setTimeout(() => setSaved(false), 2500)
  }

  if (hydrated && isLoading) {
    return (
      <div className="flex items-center gap-2 py-8 font-inter text-[13px] text-[#9B9691]">
        <Loader2 size={14} className="animate-spin" /> Loading…
      </div>
    )
  }

  // Sync name from latest org if form not yet touched
  const displayName = form.name || org?.name || ''

  return (
    <section id="general" className="scroll-mt-6">
      <div className="mb-6 flex items-center gap-3">
        <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-[#E4E1DC] bg-white">
          <Building2 size={18} className="text-[#9B9691]" />
        </div>
        <div>
          <h2 className="font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
            General
          </h2>
          <p className="font-inter text-[13px] text-[#9B9691]">
            Basic information about your workspace
          </p>
        </div>
      </div>

      <div className="space-y-4">
        <div>
          <Label>Workspace name</Label>
          <Field
            value={displayName}
            onChange={set('name')}
            placeholder="Acme Inc."
          />
        </div>

        <div>
          <Label>URL slug</Label>
          <Field
            value={form.slug || org?.slug || ''}
            onChange={set('slug')}
            placeholder="acme"
            hint="app.yourdomain.com / acme"
          />
        </div>

        <div>
          <Label>Description</Label>
          <Field
            value={form.description}
            onChange={set('description')}
            placeholder="What does your workspace do?"
          />
        </div>

        <div>
          <Label>Website</Label>
          <Field
            value={form.website}
            onChange={set('website')}
            placeholder="https://example.com"
          />
        </div>
      </div>

      <div className="mt-8 flex items-center gap-3">
        <button
          onClick={handleSave}
          className="flex items-center gap-2 rounded-xl bg-[#1C1C1A] px-5 py-2.5 font-inter text-[13px] font-medium text-white transition-colors hover:bg-[#383530] disabled:opacity-60"
        >
          {saved ? <><Check size={14} /> Saved</> : 'Save changes'}
        </button>
        {saved && (
          <span className="font-inter text-[12px] text-[#2E7D52]">
            Changes saved
          </span>
        )}
      </div>
    </section>
  )
}
