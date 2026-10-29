import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import { SmoothScroll } from "@/components/motion/smooth-scroll";
import { JsonLd } from "@/components/seo/json-ld";
import {
  HomeFaqSection,
  HeroSection,
  PortfolioSection,
  ProjectStartSection,
  ServicesSection,
  StudioIntroductionSection,
  TechnologiesSection,
} from "@/features";
import { homeFaqContent } from "@/features/faq/home-faq-data";
import type { Locale } from "@/lib/i18n/locales";

export function HomePage({ locale }: { locale: Locale }) {
  const homeFaqJsonLd = {
    "@context": "https://schema.org",
    "@type": "FAQPage",
    mainEntity: homeFaqContent[locale].faqs.map((faq) => ({
      "@type": "Question",
      name: faq.question,
      acceptedAnswer: { "@type": "Answer", text: faq.answer },
    })),
  };

  return (
    <SmoothScroll>
      <Navigation locale={locale} variant="editorial" />
      <JsonLd data={homeFaqJsonLd} />
      <main id="main-content" className="home-page min-h-screen bg-[#FCFCFA]">
        <HeroSection locale={locale} />
        <TechnologiesSection locale={locale} />
        <StudioIntroductionSection locale={locale} />
        <ServicesSection locale={locale} />
        <PortfolioSection locale={locale} />
        <HomeFaqSection locale={locale} />
        <ProjectStartSection locale={locale} />
      </main>
      <Footer locale={locale} />
    </SmoothScroll>
  );
}
