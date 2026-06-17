'use client'

import { useRouter } from 'next/navigation'
import { onboardingService, type OnboardingOrgData } from '@/components/onboarding/services/onboarding-service'
import { BrregSearch } from '../ui/BrregSearch'
import type { BrregEnhet } from '@/lib/services/brreg-service'
import { useOrganizationStepState } from './use-organization-step-state'

const inputClass =
  'w-full border border-[#D8D2C6] bg-[#F4F1EB] px-4 py-3 font-inter text-[13px] text-[#2B2B2B] outline-none transition-colors placeholder:text-[#C8C1B3] focus:border-[#2B2B2B] focus:bg-white disabled:opacity-60'
const labelClass =
  'block font-inter text-[11px] uppercase tracking-widest text-[#A09890] mb-2'

export function OrganizationStep() {
  const router = useRouter()
  const [state, dispatch] = useOrganizationStepState()
  const { loading, error, action, createData, brregEnhet, brregSkipped, joinData } = state

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    dispatch({ type: 'SUBMIT_START' })

    try {
      const data: OnboardingOrgData = {
        action,
        ...(action === 'create'
          ? {
              ...createData,
              ...(brregEnhet
                ? {
                    orgNumber: brregEnhet.organisasjonsnummer,
                    brregData: brregEnhet as unknown as Record<string, unknown>,
                  }
                : {}),
            }
          : joinData),
      }

      await onboardingService.setupOrganization(data)
      router.push('/onboarding/website')
    } catch (err: any) {
      dispatch({ type: 'SUBMIT_ERROR', payload: err.message || 'Kunne ikke opprette organisasjon' })
    } finally {
      dispatch({ type: 'SUBMIT_END' })
    }
  }

  const generateSlug = (name: string) =>
    name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  const handleNameChange = (name: string) => {
    dispatch({ type: 'UPDATE_NAME', payload: { name, slug: generateSlug(name) } })
  }

  return (
    <div>
      {/* Custom tab bar */}
      <div className="flex border-b border-[#D8D2C6] mb-6">
        {(['create', 'join'] as const).map((tab) => {
          const label = tab === 'create' ? 'Opprett ny' : 'Bli med'
          const isActive = action === tab
          return (
            <button
              key={tab}
              data-testid={`onboarding-org-tab-${tab}`}
              type="button"
              onClick={() => dispatch({ type: 'SET_ACTION', payload: tab })}
              className={`pb-3 pr-8 font-inter text-[11px] uppercase tracking-widest transition-colors -mb-px ${
                isActive
                  ? 'border-b-2 border-[#2B2B2B] text-[#2B2B2B]'
                  : 'text-[#A09890] hover:text-[#2B2B2B]'
              }`}
            >
              {label}
            </button>
          )
        })}
      </div>

      {action === 'create' && (
        <form onSubmit={handleSubmit} className="space-y-5">
          <div>
            <label htmlFor="orgName" className={labelClass}>Organisasjonsnavn *</label>
            <input
              id="orgName"
              data-testid="onboarding-org-name"
              value={createData.organizationName}
              onChange={(e) => handleNameChange(e.target.value)}
              required
              placeholder="Acme AS"
              className={inputClass}
            />
          </div>

          {createData.organizationName.length > 1 && !brregSkipped && (
            <BrregSearch
              initialQuery={createData.organizationName}
              onSelect={(enhet) => {
                dispatch({ type: 'SELECT_BRREG', payload: { enhet, name: enhet.navn, slug: generateSlug(enhet.navn) } })
              }}
              onSkip={() => {
                dispatch({ type: 'SET_BRREG_ENHET', payload: null })
                dispatch({ type: 'SET_BRREG_SKIPPED', payload: true })
              }}
            />
          )}

          <div>
            <label htmlFor="slug" className={labelClass}>URL-slug *</label>
            <input
              id="slug"
              data-testid="onboarding-org-slug"
              value={createData.organizationSlug}
              readOnly
              required
              placeholder="acme-as"
              className={inputClass}
            />
            <p className="mt-1.5 font-inter text-[11px] text-[#A09890]">
              yourapp.com/org/{createData.organizationSlug || 'slug'}
            </p>
          </div>

          <div>
            <label htmlFor="plan" className={labelClass}>Plan</label>
            <select
              id="plan"
              data-testid="onboarding-org-plan"
              value={createData.plan}
              onChange={(e) =>
                dispatch({ type: 'SET_CREATE_DATA', payload: { plan: e.target.value as 'free' | 'pro' | 'enterprise' } })
              }
              className={inputClass}
            >
              <option value="free">Gratis (1K API-kall, 5 brukere)</option>
              <option value="pro">Pro (10K API-kall, 20 brukere)</option>
              <option value="enterprise">Enterprise (ubegrenset)</option>
            </select>
          </div>

          {error && (
            <div className="border border-[#FF2E63]/30 bg-[#FF2E63]/5 px-4 py-2.5 font-inter text-[12px] text-[#FF2E63]">
              {error}
            </div>
          )}

          <div className="flex justify-between pt-2">
            <button
              type="button"
              data-testid="onboarding-org-create-back"
              onClick={async () => { await onboardingService.saveCurrentStep('profile'); router.back() }}
              className="border border-[#D8D2C6] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
            >
              Tilbake
            </button>
            <button
              type="submit"
              data-testid="onboarding-org-create-submit"
              disabled={loading}
              className="bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
            >
              {loading ? 'Oppretter...' : 'Opprett organisasjon'}
            </button>
          </div>
        </form>
      )}

      {action === 'join' && (
        <form onSubmit={handleSubmit} className="space-y-5">
          <div>
            <label htmlFor="inviteCode" className={labelClass}>Invitasjonskode *</label>
            <input
              id="inviteCode"
              data-testid="onboarding-org-invite-code"
              value={joinData.invitationCode}
              onChange={(e) => dispatch({ type: 'SET_JOIN_DATA', payload: { invitationCode: e.target.value } })}
              required
              placeholder="Skriv inn invitasjonskoden"
              className={inputClass}
            />
            <p className="mt-1.5 font-inter text-[11px] text-[#A09890]">
              Skriv inn koden du mottok for å bli med i en organisasjon
            </p>
          </div>

          {error && (
            <div className="border border-[#FF2E63]/30 bg-[#FF2E63]/5 px-4 py-2.5 font-inter text-[12px] text-[#FF2E63]">
              {error}
            </div>
          )}

          <div className="flex justify-between pt-2">
            <button
              type="button"
              data-testid="onboarding-org-join-back"
              onClick={async () => { await onboardingService.saveCurrentStep('profile'); router.back() }}
              className="border border-[#D8D2C6] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
            >
              Tilbake
            </button>
            <button
              type="submit"
              data-testid="onboarding-org-join-submit"
              disabled={loading}
              className="bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
            >
              {loading ? 'Kobler til...' : 'Bli med i organisasjon'}
            </button>
          </div>
        </form>
      )}
    </div>
  )
}
