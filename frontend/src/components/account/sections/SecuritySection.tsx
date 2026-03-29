'use client'

import { useState } from 'react'
import { Lock, Eye, EyeOff, Check, AlertCircle, Loader2 } from 'lucide-react'
import { useChangePassword, useCurrentProfile } from '../hooks/useAccount'

function PasswordInput({
  label,
  value,
  onChange,
  placeholder,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  placeholder?: string
}) {
  const [show, setShow] = useState(false)

  return (
    <div>
      <label className="mb-1.5 block font-inter text-[11px] font-medium uppercase tracking-widest text-[#9B9691]">
        {label}
      </label>
      <div className="relative">
        <input
          type={show ? 'text' : 'password'}
          value={value}
          onChange={e => onChange(e.target.value)}
          placeholder={placeholder ?? '••••••••'}
          className="block w-full rounded-xl border border-[#D8D2C6] bg-white px-3.5 py-2.5 pr-10 font-inter text-[13px] text-[#1C1C1A] placeholder:text-[#D0C9BF] outline-none transition-colors focus:border-[#1C1C1A] focus:ring-1 focus:ring-[#1C1C1A]/10"
        />
        <button
          type="button"
          onClick={() => setShow(s => !s)}
          className="absolute right-3 top-1/2 -translate-y-1/2 text-[#C8C1B3] hover:text-[#4A4A48]"
        >
          {show ? <EyeOff size={14} /> : <Eye size={14} />}
        </button>
      </div>
    </div>
  )
}

function PasswordStrength({ password }: { password: string }) {
  if (!password) return null

  const checks = [
    { label: '8+ characters',      pass: password.length >= 8 },
    { label: 'Uppercase letter',   pass: /[A-Z]/.test(password) },
    { label: 'Number',             pass: /\d/.test(password) },
    { label: 'Special character',  pass: /[^A-Za-z0-9]/.test(password) },
  ]
  const score = checks.filter(c => c.pass).length

  const barColor =
    score <= 1 ? 'bg-[#C0402A]' :
    score <= 2 ? 'bg-[#D97706]' :
    score === 3 ? 'bg-[#2E7D52]' :
                  'bg-[#1A6B3C]'

  return (
    <div className="mt-2 space-y-2">
      <div className="flex gap-1">
        {[1, 2, 3, 4].map(n => (
          <div
            key={n}
            className={`h-1 flex-1 rounded-full transition-colors ${n <= score ? barColor : 'bg-[#E4E1DC]'}`}
          />
        ))}
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1">
        {checks.map(c => (
          <span
            key={c.label}
            className={`font-inter text-[11px] ${c.pass ? 'text-[#2E7D52]' : 'text-[#C8C1B3]'}`}
          >
            {c.pass ? '✓' : '○'} {c.label}
          </span>
        ))}
      </div>
    </div>
  )
}

export function SecuritySection() {
  const { data: profile } = useCurrentProfile()
  const changePassword = useChangePassword()

  const [current, setCurrent]   = useState('')
  const [next, setNext]         = useState('')
  const [confirm, setConfirm]   = useState('')
  const [success, setSuccess]   = useState(false)
  const [validationError, setValidationError] = useState<string | null>(null)

  const handleSubmit = async () => {
    setValidationError(null)

    if (!current) { setValidationError('Enter your current password.'); return }
    if (next.length < 8) { setValidationError('New password must be at least 8 characters.'); return }
    if (next !== confirm) { setValidationError('Passwords do not match.'); return }

    try {
      await changePassword.mutateAsync({ currentPassword: current, newPassword: next })
      setSuccess(true)
      setCurrent(''); setNext(''); setConfirm('')
      setTimeout(() => setSuccess(false), 3000)
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Something went wrong.'
      setValidationError(msg)
    }
  }

  // Check if the user has a password-based provider (email/password)
  const hasPasswordAuth = !profile?.email?.endsWith('@oauth.placeholder')

  return (
    <section id="security" className="scroll-mt-6">
      <h2 className="mb-1.5 font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
        Security
      </h2>
      <p className="mb-6 font-inter text-[13px] text-[#9B9691]">
        Manage your password and authentication settings.
      </p>

      {hasPasswordAuth ? (
        <div className="space-y-4">
          <PasswordInput label="Current password" value={current} onChange={setCurrent} />
          <div>
            <PasswordInput label="New password" value={next} onChange={setNext} />
            <PasswordStrength password={next} />
          </div>
          <PasswordInput label="Confirm new password" value={confirm} onChange={setConfirm} />

          {(validationError || changePassword.isError) && (
            <div className="flex items-center gap-2 rounded-xl bg-[#FFF0ED] px-3.5 py-2.5 font-inter text-[12px] text-[#C0402A]">
              <AlertCircle size={13} />
              {validationError ?? 'Password change failed. Please try again.'}
            </div>
          )}

          <button
            onClick={handleSubmit}
            disabled={changePassword.isPending}
            className="flex items-center gap-2 rounded-xl bg-[#1C1C1A] px-5 py-2.5 font-inter text-[13px] font-medium text-white transition-colors hover:bg-[#383530] disabled:opacity-60"
          >
            {changePassword.isPending ? (
              <><Loader2 size={14} className="animate-spin" /> Updating…</>
            ) : success ? (
              <><Check size={14} /> Password updated</>
            ) : (
              <><Lock size={14} /> Update password</>
            )}
          </button>
        </div>
      ) : (
        <div className="rounded-xl border border-[#E4E1DC] bg-[#F8F7F4] px-4 py-4 font-inter text-[13px] text-[#9B9691]">
          Password authentication is not enabled for this account. You signed in using an
          external provider (OAuth). To set a password, use the &quot;Forgot password&quot; flow
          from the sign-in page.
        </div>
      )}
    </section>
  )
}
