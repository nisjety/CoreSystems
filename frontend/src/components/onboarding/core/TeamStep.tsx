'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { onboardingService, type OnboardingTeamData } from '@/components/onboarding/services/onboarding-service'

interface TeamMember {
  email: string
  role: 'admin' | 'member' | 'viewer'
}

const ROLE_LABELS: Record<string, string> = {
  admin: 'Admin',
  member: 'Medlem',
  viewer: 'Leser',
}

const inputClass =
  'w-full border border-[#D8D2C6] bg-[#F4F1EB] px-4 py-3 font-inter text-[13px] text-[#2B2B2B] outline-none transition-colors placeholder:text-[#C8C1B3] focus:border-[#2B2B2B] focus:bg-white'
const labelClass =
  'block font-inter text-[11px] uppercase tracking-widest text-[#A09890] mb-2'

export function TeamStep() {
  const router = useRouter()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [email, setEmail] = useState('')
  const [role, setRole] = useState<'admin' | 'member' | 'viewer'>('member')
  const [members, setMembers] = useState<TeamMember[]>([])

  const addMember = () => {
    if (!email) return
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
    if (!emailRegex.test(email)) {
      setError('Skriv inn en gyldig e-postadresse')
      return
    }
    if (members.some(m => m.email === email)) {
      setError('Denne e-postadressen er allerede lagt til')
      return
    }
    setMembers(prev => [...prev, { email, role }])
    setEmail('')
    setRole('member')
    setError(null)
  }

  const removeMember = (emailToRemove: string) => {
    setMembers(prev => prev.filter(m => m.email !== emailToRemove))
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    setLoading(true)

    try {
      if (members.length > 0) {
        const data: OnboardingTeamData = {
          inviteEmails: members.map(m => m.email),
          roles: members.reduce((acc, m) => ({ ...acc, [m.email]: m.role }), {}),
        }
        await onboardingService.inviteTeamMembers(data)
      }
      await onboardingService.completeOnboarding()
      router.push('/onboarding/complete')
    } catch (err: any) {
      setError(err.message || 'Kunne ikke sende invitasjoner')
    } finally {
      setLoading(false)
    }
  }

  const handleSkip = async () => {
    setLoading(true)
    try {
      await onboardingService.skipTeamInvitation()
      await onboardingService.completeOnboarding()
      router.push('/onboarding/complete')
    } catch (err: any) {
      setError(err.message || 'Kunne ikke fullføre onboarding')
    } finally {
      setLoading(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-5">
      {/* Email + role + add row */}
      <div className="flex gap-3 items-end">
        <div className="flex-1">
          <label htmlFor="email" className={labelClass}>E-postadresse</label>
          <input
            id="email"
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="kollega@eksempel.no"
            onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), addMember())}
            className={inputClass}
          />
        </div>
        <div className="w-28 shrink-0">
          <label htmlFor="role" className={labelClass}>Rolle</label>
          <select
            id="role"
            value={role}
            onChange={(e) => setRole(e.target.value as 'admin' | 'member' | 'viewer')}
            className={inputClass}
          >
            <option value="admin">Admin</option>
            <option value="member">Medlem</option>
            <option value="viewer">Leser</option>
          </select>
        </div>
        <div className="shrink-0">
          <button
            type="button"
            onClick={addMember}
            className="border border-[#D8D2C6] px-5 py-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
          >
            Legg til
          </button>
        </div>
      </div>

      {/* Member list */}
      {members.length > 0 && (
        <div>
          <p className={labelClass}>Teammedlemmer ({members.length})</p>
          <div className="space-y-2 max-h-52 overflow-y-auto">
            {members.map((member) => (
              <div
                key={member.email}
                className="flex items-center justify-between border border-[#D8D2C6] bg-[#F4F1EB] px-4 py-3"
              >
                <div className="flex items-center gap-3">
                  <span className="font-inter text-[13px] text-[#2B2B2B]">{member.email}</span>
                  <span className="border border-[#D8D2C6] bg-[#EAE6DF] px-2 py-0.5 font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
                    {ROLE_LABELS[member.role]}
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => removeMember(member.email)}
                  className="font-inter text-[11px] text-[#FF2E63] transition-opacity hover:opacity-70"
                >
                  Fjern
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {error && (
        <div className="border border-[#FF2E63]/30 bg-[#FF2E63]/5 px-4 py-2.5 font-inter text-[12px] text-[#FF2E63]">
          {error}
        </div>
      )}

      <div className="flex justify-between pt-2">
        <button
          type="button"
          onClick={() => router.back()}
          className="border border-[#D8D2C6] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
        >
          Tilbake
        </button>
        <div className="flex items-center gap-4">
          <button
            type="button"
            onClick={handleSkip}
            disabled={loading}
            className="font-inter text-[11px] uppercase tracking-widest text-[#A09890] transition-colors hover:text-[#2B2B2B] disabled:opacity-40"
          >
            Hopp over
          </button>
          <button
            type="submit"
            disabled={loading}
            className="bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
          >
            {loading ? 'Sender...' : members.length > 0 ? 'Send invitasjoner' : 'Fortsett'}
          </button>
        </div>
      </div>
    </form>
  )
}
