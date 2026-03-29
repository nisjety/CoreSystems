'use client'

import { useState, useEffect } from 'react'
import { useRouter } from 'next/navigation'
import { onboardingService, type OnboardingProfileData } from '@/components/onboarding/services/onboarding-service'
import { authService } from '@/components/auth/services/auth-service'

const inputClass =
  'w-full border border-[#D8D2C6] bg-[#F4F1EB] px-4 py-3 font-inter text-[13px] text-[#2B2B2B] outline-none transition-colors placeholder:text-[#C8C1B3] focus:border-[#2B2B2B] focus:bg-white'
const labelClass =
  'block font-inter text-[11px] uppercase tracking-widest text-[#A09890] mb-2'

export function ProfileStep() {
  const router = useRouter()
  const [loading, setLoading] = useState(false)
  const [initializing, setInitializing] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [formData, setFormData] = useState<OnboardingProfileData>({
    firstName: '',
    lastName: '',
    displayName: '',
    phoneNumber: '',
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    jobTitle: '',
    department: '',
  })

  // Pre-fill form with OAuth user data
  useEffect(() => {
    async function loadUserData() {
      try {
        const user = await authService.getCurrentUser()
        if (user && user.name) {
          // Split full name into first and last name
          const nameParts = user.name.trim().split(' ')
          const firstName = nameParts[0] || ''
          const lastName = nameParts.slice(1).join(' ') || ''
          
          setFormData(prev => ({
            ...prev,
            firstName,
            lastName,
            displayName: user.name,
          }))
          
          console.log('✅ Pre-filled profile with OAuth data:', { firstName, lastName })
        }
      } catch (err) {
        console.error('Failed to load user data:', err)
      } finally {
        setInitializing(false)
      }
    }
    
    loadUserData()
  }, [])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    setLoading(true)

    try {
      await onboardingService.completeProfile(formData)
      router.push('/onboarding/organization')
    } catch (err: any) {
      setError(err.message || 'Failed to save profile')
    } finally {
      setLoading(false)
    }
  }

  const handleChange = (field: keyof OnboardingProfileData, value: string) => {
    setFormData(prev => ({ ...prev, [field]: value }))
  }

  if (initializing) {
    return (
      <div className="space-y-4 animate-pulse">
        <div className="h-12 bg-[#EAE6DF]" />
        <div className="h-12 bg-[#EAE6DF]" />
        <div className="h-12 bg-[#EAE6DF]" />
      </div>
    )
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-5">
      <div className="grid grid-cols-2 gap-4">
        <div>
          <label htmlFor="firstName" className={labelClass}>Fornavn *</label>
          <input
            id="firstName"
            value={formData.firstName}
            onChange={(e) => handleChange('firstName', e.target.value)}
            required
            placeholder="Ola"
            className={inputClass}
          />
        </div>
        <div>
          <label htmlFor="lastName" className={labelClass}>Etternavn *</label>
          <input
            id="lastName"
            value={formData.lastName}
            onChange={(e) => handleChange('lastName', e.target.value)}
            required
            placeholder="Nordmann"
            className={inputClass}
          />
        </div>
      </div>

      <div>
        <label htmlFor="displayName" className={labelClass}>Visningsnavn</label>
        <input
          id="displayName"
          value={formData.displayName}
          onChange={(e) => handleChange('displayName', e.target.value)}
          placeholder="Hva vil du bli kalt?"
          className={inputClass}
        />
      </div>

      <div className="grid grid-cols-2 gap-4">
        <div>
          <label htmlFor="jobTitle" className={labelClass}>Stilling</label>
          <input
            id="jobTitle"
            value={formData.jobTitle}
            onChange={(e) => handleChange('jobTitle', e.target.value)}
            placeholder="Programvareutvikler"
            className={inputClass}
          />
        </div>
        <div>
          <label htmlFor="department" className={labelClass}>Avdeling</label>
          <input
            id="department"
            value={formData.department}
            onChange={(e) => handleChange('department', e.target.value)}
            placeholder="Utvikling"
            className={inputClass}
          />
        </div>
      </div>

      <div>
        <label htmlFor="phoneNumber" className={labelClass}>Telefonnummer</label>
        <input
          id="phoneNumber"
          type="tel"
          value={formData.phoneNumber}
          onChange={(e) => handleChange('phoneNumber', e.target.value)}
          placeholder="+47 000 00 000"
          className={inputClass}
        />
      </div>

      <div>
        <label htmlFor="timezone" className={labelClass}>Tidssone</label>
        <input
          id="timezone"
          value={formData.timezone}
          onChange={(e) => handleChange('timezone', e.target.value)}
          placeholder="Europe/Oslo"
          className={inputClass}
        />
      </div>

      {error && (
        <div className="border border-[#FF2E63]/30 bg-[#FF2E63]/5 px-4 py-2.5 font-inter text-[12px] text-[#FF2E63]">
          {error}
        </div>
      )}

      <div className="flex justify-end pt-2">
        <button
          type="submit"
          disabled={loading}
          className="bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
        >
          {loading ? 'Lagrer...' : 'Fortsett'}
        </button>
      </div>
    </form>
  )
}
