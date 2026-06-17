'use client'

import { useState } from 'react'
import { X, UserPlus, Loader2, AlertCircle } from 'lucide-react'
import type { MemberRole } from './services/team-service'

interface AddMemberModalProps {
  onClose: () => void
  onInvite: (email: string, role: MemberRole) => Promise<void>
}

const ROLE_OPTIONS: { value: MemberRole; label: string; description: string }[] = [
  { value: 'admin',  label: 'Admin',  description: 'Can manage team members and settings.' },
  { value: 'member', label: 'Member', description: 'Can use all workspace features.' },
  { value: 'viewer', label: 'Viewer', description: 'Read-only access.' },
]

export function AddMemberModal({ onClose, onInvite }: AddMemberModalProps) {
  const [email, setEmail]     = useState('')
  const [role, setRole]       = useState<MemberRole>('member')
  const [isBusy, setIsBusy]   = useState(false)
  const [error, setError]     = useState<string | null>(null)

  const handleSubmit = async () => {
    setError(null)
    const trimmed = email.trim().toLowerCase()
    if (!trimmed) { setError('Enter an email address.'); return }
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(trimmed)) {
      setError('Enter a valid email address.')
      return
    }
    setIsBusy(true)
    try {
      await onInvite(trimmed, role)
      onClose()
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Invitation failed. Please try again.')
    } finally {
      setIsBusy(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[#1C1C1A]/30 p-4 backdrop-blur-sm">
      <div className="w-full max-w-[420px] overflow-hidden rounded-[12px] border border-[#E4E1DC] bg-white shadow-[0_24px_48px_rgba(0,0,0,0.12)]">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[#E4E1DC] px-6 py-5">
          <div className="flex items-center gap-2.5">
            <UserPlus size={15} className="text-[#9B9691]" />
            <h2 className="font-inter text-[15px] font-semibold text-[#1C1C1A]">
              Add team member
            </h2>
          </div>
          <button
            onClick={onClose}
            className="flex h-7 w-7 items-center justify-center rounded-[6px] text-[#C8C1B3] hover:bg-[#F4F1EB] hover:text-[#1C1C1A] transition-colors"
          >
            <X size={14} />
          </button>
        </div>

        {/* Body */}
        <div className="space-y-5 px-6 py-6">
          {/* Email */}
          <div>
            <label htmlFor="add-member-email" className="mb-1.5 block font-inter text-[11px] font-medium uppercase tracking-widest text-[#9B9691]">
              Email address
            </label>
            <input
              id="add-member-email"
              type="email"
              value={email}
              onChange={e => setEmail(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') handleSubmit() }}
              placeholder="colleague@company.com"
              className="block w-full rounded-xl border border-[#D8D2C6] bg-white px-3.5 py-2.5 font-inter text-[13px] text-[#1C1C1A] placeholder:text-[#C8C1B3] outline-none transition-colors focus:border-[#1C1C1A] focus:ring-1 focus:ring-[#1C1C1A]/10"
            />
          </div>

          {/* Role */}
          <div>
            <p className="mb-2 block font-inter text-[11px] font-medium uppercase tracking-widest text-[#9B9691]">
              Role
            </p>
            <div className="space-y-2">
              {ROLE_OPTIONS.map(opt => (
                <label
                  key={opt.value}
                  className={[
                    'flex cursor-pointer items-start gap-3 rounded-xl border px-3.5 py-3 transition-colors',
                    role === opt.value
                      ? 'border-[#1C1C1A] bg-[#F8F7F4]'
                      : 'border-[#E4E1DC] bg-white hover:border-[#C8C1B3]',
                  ].join(' ')}
                >
                  <input
                    type="radio"
                    name="role"
                    value={opt.value}
                    checked={role === opt.value}
                    onChange={() => setRole(opt.value)}
                    className="mt-0.5 accent-[#1C1C1A]"
                  />
                  <div>
                    <p className="font-inter text-[13px] font-medium text-[#1C1C1A]">{opt.label}</p>
                    <p className="font-inter text-[11px] text-[#9B9691]">{opt.description}</p>
                  </div>
                </label>
              ))}
            </div>
          </div>

          {/* Error */}
          {error && (
            <div className="flex items-center gap-2 rounded-xl bg-[#FFF0ED] px-3.5 py-2.5 font-inter text-[12px] text-[#C0402A]">
              <AlertCircle size={13} />
              {error}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 border-t border-[#E4E1DC] px-6 py-4">
          <button
            onClick={onClose}
            className="rounded-xl px-4 py-2.5 font-inter text-[13px] text-[#9B9691] hover:text-[#1C1C1A] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={isBusy}
            className="flex items-center gap-2 rounded-xl bg-[#1C1C1A] px-5 py-2.5 font-inter text-[13px] font-medium text-white transition-colors hover:bg-[#383530] disabled:opacity-60"
          >
            {isBusy ? (
              <><Loader2 size={13} className="animate-spin" /> Sending invite…</>
            ) : (
              <><UserPlus size={13} /> Send invite</>
            )}
          </button>
        </div>
      </div>
    </div>
  )
}
