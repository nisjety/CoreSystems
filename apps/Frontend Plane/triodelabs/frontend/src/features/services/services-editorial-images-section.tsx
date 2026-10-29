import Image from "next/image";
import type { Locale } from "@/lib/i18n/locales";
import { servicesPageCopy } from "./services-page-data";
import { ServicesSectionLabel } from "./services-section-label";

const images = [
  { src: "/images/Triodelab-bilder/Jegogimasittende.jpg" },
  { src: "/images/Triodelab-bilder/JackogIma.JPG" },
  { src: "/images/Triodelab-bilder/JackStaende.jpg", position: "50% 15%" },
] as const;

export function ServicesEditorialImagesSection({
  locale,
}: {
  locale: Locale;
}) {
  const copy = servicesPageCopy[locale].editorial;

  return (
    <section
      className="px-[clamp(24px,5vw,80px)] py-[clamp(80px,10vw,150px)]"
      aria-labelledby="services-images-heading"
    >
      <div className="grid gap-10 lg:grid-cols-12">
        <div className="lg:col-span-2">
          <ServicesSectionLabel>{copy.label}</ServicesSectionLabel>
        </div>
        <h2
          id="services-images-heading"
          className="max-w-[12ch] font-manrope text-[clamp(2.9rem,5vw,5.5rem)] font-light leading-[0.95] tracking-[-0.07em] lg:col-span-6"
        >
          {copy.heading}
        </h2>
      </div>

      <div className="mt-[clamp(56px,7vw,104px)] grid gap-x-[clamp(16px,2vw,32px)] gap-y-12 md:grid-cols-3">
        {images.map((image, index) => (
          <figure
            key={image.src}
            className={index === 1 ? "md:mt-[clamp(48px,6vw,96px)]" : undefined}
          >
            <div className="relative aspect-[4/5] overflow-hidden bg-[#D7D7D2]">
              <Image
                src={image.src}
                alt={copy.notes[index].alt}
                fill
                sizes="(min-width: 768px) 33vw, 100vw"
                className="object-cover"
                style={
                  "position" in image
                    ? { objectPosition: image.position }
                    : undefined
                }
              />
            </div>
            <figcaption className="mt-4 grid gap-2">
              <span className="font-label text-[0.67rem] uppercase tracking-[0.14em] text-[#8C8C87]">
                {copy.notes[index].label}
              </span>
              <span className="max-w-[28rem] font-body text-[0.93rem] font-light leading-[1.45] text-[#656560]">
                {copy.notes[index].note}
              </span>
            </figcaption>
          </figure>
        ))}
      </div>
    </section>
  );
}
