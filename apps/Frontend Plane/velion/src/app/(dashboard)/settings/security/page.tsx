import { SecuritySection } from '@/components/account/sections/SecuritySection'
import { SettingsSectionFrame } from '@/components/settings/SettingsSectionFrame'

export default function SettingsSecurityPage() {
  return (
    <SettingsSectionFrame
      eyebrow="Access & control"
      title="Security"
      description="Review password security and sign-in protections for your Verevon account."
    >
      <section className="rounded-[28px] border border-black/8 bg-[#FCFBF8]/96 px-6 py-6 shadow-[0_18px_40px_rgba(22,20,17,0.06)]">
        <SecuritySection />
      </section>
    </SettingsSectionFrame>
  )
}
