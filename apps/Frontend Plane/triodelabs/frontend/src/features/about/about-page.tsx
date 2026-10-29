import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import { TeamSection } from "@/features/team/team-section";
import { AboutCtaSection } from "./about-cta-section";
import { AboutHeroSection } from "./about-hero-section";
import { AboutMethodSection } from "./about-method-section";
import { AboutPrinciplesSection } from "./about-principles-section";
import type { Locale } from "@/lib/i18n/locales";

export function AboutPage({ locale }: { locale: Locale }) {
  return (
    <>
      <Navigation locale={locale} theme="dark" variant="editorial" />
      <main id="main-content" className="bg-[#FCFCFA] text-[#161616]">
        <AboutHeroSection locale={locale} />
        <AboutPrinciplesSection locale={locale} />
        <TeamSection locale={locale} />
        <AboutMethodSection locale={locale} />
        <AboutCtaSection locale={locale} />
      </main>
      <Footer locale={locale} />
    </>
  );
}
