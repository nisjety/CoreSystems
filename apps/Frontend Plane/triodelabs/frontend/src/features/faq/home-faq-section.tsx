import type { Locale } from "@/lib/i18n/locales";
import { homeFaqContent } from "./home-faq-data";

export function HomeFaqSection({ locale }: { locale: Locale }) {
  const content = homeFaqContent[locale];

  return (
    <section
      aria-labelledby="faq-heading"
      className="bg-[#FCFCFA] py-[var(--home-section-space)] text-[#161616]"
      id="faq"
    >
      <div className="mx-auto grid w-[calc(100%-48px)] max-w-[1600px] gap-12 sm:w-[92vw] lg:grid-cols-12 lg:gap-8">
        <header className="lg:col-span-4">
          <h2 className="home-section-heading max-w-[11ch]" id="faq-heading">
            {content.heading}
          </h2>
          <p className="home-body mb-0 mt-7 max-w-[22rem] text-[#626260]">
            {content.intro}
          </p>
        </header>

        <div className="lg:col-span-7 lg:col-start-6">
          {content.faqs.map(({ answer, question }) => (
            <details
              className="group border-b border-[#D7D6D0] first:border-t"
              key={question}
            >
              <summary className="home-body flex cursor-pointer list-none items-center justify-between gap-6 py-6 marker:hidden focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#161616] [&::-webkit-details-marker]:hidden">
                <span>{question}</span>
                <span
                  aria-hidden="true"
                  className="shrink-0 text-[1.5rem] leading-none transition-transform duration-200 group-open:rotate-45"
                >
                  +
                </span>
              </summary>
              <p className="home-body mb-0 max-w-[40rem] pb-6 pr-12 text-[#626260]">
                {answer}
              </p>
            </details>
          ))}
        </div>
      </div>
    </section>
  );
}
