'use client'

import { useState } from 'react'
import { Lock, Eye, EyeOff, Check, AlertCircle, Loader2 } from 'lucide-react'
import { useChangePassword, useCurrentProfile } from '../hooks/useAccount'
import {
  AccountInput,
  AccountSection,
  FieldLabel,
  InlineNotice,
  PrimaryButton,
} from './AccountFormPrimitives'
import { useSecurityState } from './use-security-state'

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
      <FieldLabel>{label}</FieldLabel>
      <div className="relative">
        <AccountInput
          type={show ? 'text' : 'password'}
          value={value}
          onChange={e => onChange(e.target.value)}
          placeholder={placeholder ?? '••••••••'}
          className="pr-12"
        />
        <button
          type="button"
          onClick={() => setShow(s => !s)}
          className="absolute right-4 top-1/2 -translate-y-1/2 text-[#A5ACB8] transition hover:text-[#4A4A48]"
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
    <div className="mt-3 space-y-3">
      <div className="flex gap-1">
        {[1, 2, 3, 4].map(n => (
          <div
            key={n}
            className={`h-1.5 flex-1 rounded-full transition-colors ${n <= score ? barColor : 'bg-[#E7E3DA]'}`}
          />
        ))}
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1">
        {checks.map(c => (
          <span
            key={c.label}
            className={`text-sm ${c.pass ? 'text-[#2E7D52]' : 'text-black/34'}`}
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

  const [state, dispatch] = useSecurityState()
  const { current, next, confirm, success, validationError } = state

  const handleSubmit = async () => {
    dispatch({ type: 'SET_VALIDATION_ERROR', payload: null })

    if (!current) { dispatch({ type: 'SET_VALIDATION_ERROR', payload: 'Enter your current password.' }); return }
    if (next.length < 8) { dispatch({ type: 'SET_VALIDATION_ERROR', payload: 'New password must be at least 8 characters.' }); return }
    if (next !== confirm) { dispatch({ type: 'SET_VALIDATION_ERROR', payload: 'Passwords do not match.' }); return }

    try {
      await changePassword.mutateAsync({ currentPassword: current, newPassword: next })
      dispatch({ type: 'SUBMIT_SUCCESS' })
      setTimeout(() => dispatch({ type: 'CLEAR_SUCCESS' }), 3000)
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Something went wrong.'
      dispatch({ type: 'SET_VALIDATION_ERROR', payload: msg })
    }
  }

  // Check if the user has a password-based provider (email/password)
  const hasPasswordAuth = !profile?.email?.endsWith('@oauth.placeholder')

  return (
    <AccountSection
      id="security"
      title="Security"
      description=""
    >
      {hasPasswordAuth ? (
        <div className="space-y-5">
          <PasswordInput label="Current password" value={current} onChange={(v) => dispatch({ type: 'SET_CURRENT', payload: v })} />
          <div>
            <PasswordInput label="New password" value={next} onChange={(v) => dispatch({ type: 'SET_NEXT', payload: v })} />
            <PasswordStrength password={next} />
          </div>
          <PasswordInput label="Confirm new password" value={confirm} onChange={(v) => dispatch({ type: 'SET_CONFIRM', payload: v })} />

          {(validationError || changePassword.isError) && (
            <InlineNotice tone="danger">
              <span className="inline-flex items-center gap-2">
                <AlertCircle size={14} />
                {validationError ?? 'Password change failed. Please try again.'}
              </span>
            </InlineNotice>
          )}

          <PrimaryButton
            onClick={handleSubmit}
            disabled={changePassword.isPending}
            className="mt-2"
          >
            {changePassword.isPending ? (
              <>
                <Loader2 size={15} className="animate-spin" />
                Updating password
              </>
            ) : success ? (
              <>
                <Check size={15} />
                Password updated
              </>
            ) : (
              <>
                <Lock size={15} />
                Update password
              </>
            )}
          </PrimaryButton>
        </div>
      ) : (
        <InlineNotice tone="neutral">
          Password authentication is not enabled for this account. You signed in using an
          external provider, so use the forgot-password flow on the sign-in page if you want to
          add a password later.
        </InlineNotice>
      )}
    </AccountSection>
  )
}
