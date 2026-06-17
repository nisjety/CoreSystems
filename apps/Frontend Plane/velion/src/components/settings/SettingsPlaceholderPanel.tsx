import Link from 'next/link'
import { ArrowRight } from 'lucide-react'

export function SettingsPlaceholderPanel({
  title,
  copy,
  ctaHref,
  ctaLabel,
}: {
  title: string
  copy: string
  ctaHref?: string
  ctaLabel?: string
}) {
  return (
    <section>
      <h2 className="mb-5 text-[15px] font-semibold text-[#111111]">
        {title}
      </h2>
      <p className="text-[13px] leading-6 text-[#6B7280]">
        {copy}
      </p>
      {ctaHref && ctaLabel ? (
        <Link
          href={ctaHref}
          className="mt-4 inline-flex items-center gap-1.5 text-[13px] font-medium text-[#111111] hover:underline"
        >
          {ctaLabel}
          <ArrowRight size={13} strokeWidth={2} />
        </Link>
      ) : null}
    </section>
  )
}
