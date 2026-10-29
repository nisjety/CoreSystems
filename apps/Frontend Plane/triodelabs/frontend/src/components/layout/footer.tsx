import Link from "next/link";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { footerContent } from "./footer-content";

export function Footer({ locale }: { locale: Locale }) {
  const currentYear = new Date().getFullYear();
  const content = footerContent[locale];

  return (
    <footer
      className="relative overflow-hidden bg-[#FCFCFA] text-[#161616] lg:flex lg:min-h-[68svh]"
      id="footer"
    >
      <div className="px-[clamp(24px,4vw,56px)] pb-0 pt-[clamp(32px,3vw,48px)] lg:flex lg:min-h-[68svh] lg:w-full lg:flex-col">
        <div className="grid gap-[clamp(48px,6vw,112px)] pb-[clamp(56px,8vh,112px)] min-[901px]:grid-cols-[minmax(180px,0.9fr)_minmax(0,3.1fr)] lg:flex-1">
          <div className="grid content-start gap-8 min-[901px]:pt-10">
            <Link
              aria-label={content.home}
              className="w-fit font-manrope text-[clamp(2rem,2.4vw,3rem)] font-light leading-none tracking-[-0.055em] text-[#161616] transition-opacity hover:opacity-60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
              href={localizeHref("/", locale)}
            >
              TRIODELAB
            </Link>

            <p className="m-0 max-w-[20rem] font-manrope text-[clamp(0.9rem,0.95vw,1.08rem)] font-light uppercase leading-[1.32] tracking-[0.02em] text-[#626260]">
              {content.tagline}
            </p>
          </div>

          <div className="grid grid-cols-2 gap-x-[clamp(24px,3vw,56px)] gap-y-8 max-[560px]:grid-cols-1 min-[661px]:grid-cols-3">
            {content.columns.map((column) => (
              <nav
                aria-label={content.navLabel(column.label)}
                className="grid content-start"
                key={column.label}
              >
                {column.links.map((link) => (
                  <Link
                    className="py-[18px] font-manrope text-[clamp(0.88rem,0.9vw,1.05rem)] font-normal uppercase leading-none tracking-[0.08em] text-[#626260] transition-colors hover:text-[#161616] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                    href={localizeHref(link.href, locale)}
                    key={link.label}
                  >
                    {link.label}
                  </Link>
                ))}
              </nav>
            ))}

            <div className="col-span-1 mt-[clamp(24px,2.5vw,40px)]">
              <h2 className="mb-6 font-manrope text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-[#161616]">
                {content.officeHeading}
              </h2>
              <address className="m-0 not-italic font-manrope text-[clamp(1rem,1vw,1.18rem)] font-light leading-[1.35] text-[#626260]">
                {content.address.map((line, index) => (
                  <span key={line}>
                    {index > 0 && <br />}
                    {line}
                  </span>
                ))}
              </address>
            </div>

            <div className="col-span-1 mt-[clamp(24px,2.5vw,40px)]">
              <h2 className="mb-6 font-manrope text-[clamp(0.84rem,0.86vw,1rem)] font-normal uppercase tracking-[0.08em] text-[#161616]">
                {content.contactHeading}
              </h2>
              <address className="m-0 not-italic font-manrope text-[clamp(1rem,1vw,1.18rem)] font-light leading-[1.35] text-[#626260]">
                <a
                  className="inline-flex min-h-11 items-center transition-colors hover:text-[#161616] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                  href="mailto:post@triodelab.no"
                >
                  post@triodelab.no
                </a>
                <br />
                <a
                  className="inline-flex min-h-11 items-center transition-colors hover:text-[#161616] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                  href="tel:+4798846460"
                >
                  +47 988 46 460
                </a>
              </address>
            </div>


            <Link
              className="col-span-1 self-end justify-self-start font-manrope text-[clamp(0.94rem,0.95vw,1.08rem)] font-light text-[#626260] transition-colors hover:text-[#161616] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616] min-[661px]:col-start-4"
              href={localizeHref("/", locale)}
            >
              <span aria-hidden="true">↑</span> {content.backHome}
            </Link>
          </div>
        </div>

        <div className="grid gap-[clamp(48px,6vw,112px)] py-4 min-[901px]:grid-cols-[minmax(180px,0.9fr)_minmax(0,3.1fr)]">
          <div />

          <div className="grid items-center gap-4 font-manrope text-[clamp(0.88rem,0.9vw,1rem)] font-light text-[#626260] min-[661px]:grid-cols-2 min-[661px]:gap-6">
            <span>{content.disciplines}</span>
            <small className="justify-self-start font-manrope text-[clamp(0.88rem,0.9vw,1rem)] font-light min-[661px]:justify-self-end">
              © {currentYear} TriodeLab
            </small>
          </div>
        </div>
        <nav
          aria-label={content.legalLabel}
          className="flex flex-wrap gap-x-6 gap-y-3 border-t border-[#D9D9D4] py-5 font-manrope text-sm text-[#626260]"
        >
          {content.legal.map((link) => (
            <Link
              key={link.href}
              href={localizeHref(link.href, locale)}
              className="underline-offset-4 hover:underline"
            >
              {link.label}
            </Link>
          ))}
        </nav>
      </div>
    </footer>
  );
}
