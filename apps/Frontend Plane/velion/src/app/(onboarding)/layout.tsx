import { OnboardingPage } from '@/components/onboarding/page'

export default function OnboardingLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return <OnboardingPage>{children}</OnboardingPage>
}
