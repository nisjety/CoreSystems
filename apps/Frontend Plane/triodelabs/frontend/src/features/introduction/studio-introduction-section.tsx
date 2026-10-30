import type { Locale } from "@/lib/i18n/locales";
import { studioIntroductionContent } from "./studio-introduction-content";

export function StudioIntroductionSection({ locale }: { locale: Locale }) {
  const content = studioIntroductionContent[locale];
  const [firstSentence, secondSentence] = content.heading.split(". ");
  const [firstMutedWord, ...firstSentenceWords] = firstSentence.split(" ");
  const [secondMutedWord, ...secondSentenceWords] = secondSentence.split(" ");

  return (
    <section
      aria-labelledby="studio-introduction-heading"
      className="scroll-mt-[92px] bg-[#FCFCFA] py-[clamp(112px,11vw,180px)] text-[#161616]"
      id="introduksjon"
    >
      <div className="mx-auto w-[calc(100%-48px)] max-w-[1600px] sm:w-[92vw]">
        <h2
          className="home-section-heading mx-auto max-w-[18ch] text-balance text-center"
          id="studio-introduction-heading"
        >
          <span className="text-[#73736F]">{firstMutedWord}</span>{" "}
          {firstSentenceWords.join(" ")}.
          <br />
          <span className="text-[#73736F]">{secondMutedWord}</span>{" "}
          {secondSentenceWords.join(" ")}
        </h2>
        <div className="mt-[clamp(64px,8vw,120px)] grid gap-14 border-t border-[#D7D6D0] pt-[clamp(28px,4vw,56px)] lg:grid-cols-12 lg:gap-12">
          <p className="home-ui m-0 max-w-[16rem] border-l border-[#91BDCA] pl-5 text-[#626260] lg:col-span-3">
            {content.aside}
          </p>
          <div className="space-y-7 lg:col-span-6 lg:col-start-7">
            {content.paragraphs.map((paragraph) => (
              <p
                className="home-body m-0 max-w-[36rem] text-[#545861]"
                key={paragraph}
              >
                {paragraph}
              </p>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
