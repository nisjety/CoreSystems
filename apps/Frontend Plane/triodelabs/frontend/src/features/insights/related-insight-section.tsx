import Link from "next/link";
import { ArrowRight } from "@/components/icons";

type RelatedInsightSectionProps = {
  description: string;
  href: string;
  title: string;
};

export function RelatedInsightSection({
  description,
  href,
  title,
}: RelatedInsightSectionProps) {
  return (
    <section
      aria-labelledby="related-insight-heading"
      className="border-b border-[#E1E1DD] bg-[#F7F7F4] px-[clamp(32px,5.4vw,96px)] py-[clamp(56px,8vw,132px)]"
    >
      <div className="grid gap-10 lg:grid-cols-[minmax(0,0.72fr)_minmax(18rem,0.95fr)] lg:gap-20">
        <p className="m-0 font-label text-[0.66rem] uppercase tracking-[0.16em] text-[#626260]">
          Fra innsikt
        </p>
        <div>
          <h2
            className="m-0 max-w-[14ch] font-manrope text-[clamp(2.25rem,4vw,4.8rem)] font-light leading-[0.9] tracking-[-0.07em]"
            id="related-insight-heading"
          >
            {title}
          </h2>
          <p className="mb-0 mt-6 max-w-[39rem] font-body text-[1.03rem] font-light leading-[1.55] text-[#626260]">
            {description}
          </p>
          <Link
            className="mt-8 inline-flex items-center gap-3 font-label text-[0.7rem] uppercase tracking-[0.12em] transition-opacity hover:opacity-55 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
            href={href}
          >
            Les guiden <ArrowRight aria-hidden="true" className="h-4 w-4" />
          </Link>
        </div>
      </div>
    </section>
  );
}
