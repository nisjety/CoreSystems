import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import type { Locale } from "@/lib/i18n/locales";
import { ServicesApproachSection } from "./services-approach-section";
import { ServicesCtaSection } from "./services-cta-section";
import { ServicesEditorialImagesSection } from "./services-editorial-images-section";
import { ServicesHeroSection } from "./services-hero-section";
import { ServicesOverviewSection } from "./services-overview-section";

export function ServicesPage({ locale }: { locale: Locale }) {
  return (
    <>
      <Navigation locale={locale} variant="editorial" />
      <main id="main-content" className="bg-[#FCFCFA] text-[#161616]">
        <ServicesHeroSection locale={locale} />
        <ServicesOverviewSection locale={locale} />
        <ServicesApproachSection locale={locale} />
        <ServicesEditorialImagesSection locale={locale} />
        <ServicesCtaSection locale={locale} />
      </main>
      <Footer locale={locale} />
    </>
  );
}
