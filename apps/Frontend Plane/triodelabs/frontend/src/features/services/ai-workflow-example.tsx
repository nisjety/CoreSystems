import Image from "next/image";
import Link from "next/link";
import type { Locale, Localized } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { ownedProductsByLocale } from "../portfolio/portfolio-content";

const content: Localized<{
  body: string;
  explore: (product: string) => string;
  heading: string;
  imageAlt: (product: string) => string;
  marker: string;
  platformAfter: string;
  platformBefore: string;
  platformLink: string;
  productsBody: string;
  productsHeading: string;
  steps: readonly { description: string; title: string }[];
}> = {
  nb: {
    marker: "Praktisk eksempel / Fra spørsmål til handling",
    heading: "En arbeidsflyt med mennesker i beslutningen.",
    body: "Dette er et eksempel på hvordan en pilot kan avgrenses. Kilder, godkjenning og eventuell kobling mellom systemer må tilpasses oppgaven og avtales for det konkrete prosjektet.",
    steps: [
      {
        title: "Et spørsmål kommer inn",
        description:
          "En kunde eller kollega trenger hjelp. Arbeidsflyten finner relevant, godkjent informasjon fra kildene virksomheten har valgt.",
      },
      {
        title: "Et svar foreslås",
        description:
          "KI lager et utkast med grunnlag som kan kontrolleres. Mangler det informasjon, sendes oppgaven til en person som kan vurdere den.",
      },
      {
        title: "En person godkjenner neste steg",
        description:
          "Før en handling som endrer data eller sender noe videre, kontrollerer en ansvarlig innholdet. En eventuell integrasjon utfører bare de handlingene som er avtalt.",
      },
    ],
    productsHeading: "Erfaring fra våre egne produkter.",
    productsBody:
      "Vi utvikler Agenci for samtaler basert på virksomhetens kunnskap og Verevon for arbeidsflyter med kontrollerte handlinger. Produktene gir et konkret utgangspunkt når vi vurderer hva som kan gjenbrukes, og hva som krever egen utvikling.",
    imageAlt: (product) => `Produktvisning av ${product} på en dataskjerm`,
    explore: (product) => `Utforsk ${product}`,
    platformBefore: "Les mer om",
    platformLink: "TriodeLabs produktplattform",
    platformAfter:
      "og hvordan produkter og konsulentarbeid kan inngå i samme leveranse.",
  },
  en: {
    marker: "Practical example / From question to action",
    heading: "A workflow with people in the decision.",
    body: "This is an example of how a pilot can be scoped. Sources, approval and any connections between systems must be adapted to the task and agreed for the specific project.",
    steps: [
      {
        title: "A question comes in",
        description:
          "A customer or colleague needs help. The workflow finds relevant, approved information from the sources your organisation has chosen.",
      },
      {
        title: "An answer is suggested",
        description:
          "AI writes a draft with a basis that can be checked. If information is missing, the task is passed to a person who can assess it.",
      },
      {
        title: "A person approves the next step",
        description:
          "Before any action that changes data or sends something on, a responsible person checks the content. Any integration only carries out the actions that have been agreed.",
      },
    ],
    productsHeading: "Experience from our own products.",
    productsBody:
      "We develop Agenci for conversations based on your organisation’s knowledge, and Verevon for workflows with controlled actions. The products give us a concrete starting point when we assess what can be reused and what needs to be built from scratch.",
    imageAlt: (product) => `Product view of ${product} on a computer screen`,
    explore: (product) => `Explore ${product}`,
    platformBefore: "Read more about",
    platformLink: "TriodeLab’s product platform",
    platformAfter:
      "and how products and consulting work can be part of the same delivery.",
  },
};

export function AiWorkflowExample({ locale }: { locale: Locale }) {
  const copy = content[locale];

  return (
    <section
      aria-labelledby="workflow-heading"
      className="border-b border-[#E1E1DD] px-[clamp(28px,5.4vw,96px)] py-[clamp(56px,8vw,112px)]"
    >
      <p className="m-0 font-label text-[0.66rem] font-light uppercase tracking-[0.16em] text-[#626260]">
        {copy.marker}
      </p>
      <h2
        id="workflow-heading"
        className="mb-0 mt-10 max-w-[18ch] font-manrope text-[clamp(2.4rem,4vw,4.6rem)] font-light leading-[0.98] tracking-[-0.065em]"
      >
        {copy.heading}
      </h2>
      <p className="mb-0 mt-7 max-w-[45rem] font-body text-[1.08rem] font-light leading-[1.6] text-[#4D4D4A]">
        {copy.body}
      </p>
      <ol className="mb-0 mt-12 grid list-none gap-10 p-0 lg:grid-cols-3">
        {copy.steps.map((step, index) => (
          <li key={step.title} className="border-t border-[#D8D8D3] pt-6">
            <span
              aria-hidden="true"
              className="font-label text-xs text-[#626260]"
            >
              0{index + 1}
            </span>
            <h3 className="mb-0 mt-5 font-manrope text-[1.55rem] font-light leading-[1.1] tracking-[-0.045em]">
              {step.title}
            </h3>
            <p className="mb-0 mt-5 font-body text-base font-light leading-[1.65] text-[#4D4D4A]">
              {step.description}
            </p>
          </li>
        ))}
      </ol>
      <div className="mt-16 border-t border-[#D8D8D3] pt-10">
        <h3 className="m-0 font-manrope text-[1.75rem] font-light tracking-[-0.045em]">
          {copy.productsHeading}
        </h3>
        <p className="mb-0 mt-4 max-w-[45rem] font-body text-base font-light leading-[1.65] text-[#4D4D4A]">
          {copy.productsBody}
        </p>
        <div className="mt-8 grid gap-10 md:grid-cols-2">
          {ownedProductsByLocale[locale].map((product) => (
            <div key={product.title}>
              <div className="relative aspect-[16/10] overflow-hidden bg-[#F1F2EF]">
                <Image
                  alt={copy.imageAlt(product.title)}
                  src={product.image}
                  fill
                  sizes="(max-width: 767px) 100vw, 45vw"
                  className="object-cover"
                />
              </div>
              <h4 className="mb-0 mt-6 font-manrope text-[1.5rem] font-light tracking-[-0.04em]">
                {product.title}
              </h4>
              <p className="mb-0 mt-3 max-w-[34rem] font-body text-base font-light leading-[1.6] text-[#4D4D4A]">
                {product.description}
              </p>
              <a
                href={product.href}
                className="mt-5 inline-block font-body text-sm underline underline-offset-4 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4"
              >
                {copy.explore(product.title)}
              </a>
            </div>
          ))}
        </div>
        <p className="mb-0 mt-8 font-body text-base font-light leading-[1.6] text-[#4D4D4A]">
          {copy.platformBefore}{" "}
          <Link
            href={localizeHref("/plattform", locale)}
            className="text-[#161616] underline underline-offset-4 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4"
          >
            {copy.platformLink}
          </Link>{" "}
          {copy.platformAfter}
        </p>
      </div>
    </section>
  );
}
