import type { ReactNode } from 'react'

interface SettingsSectionFrameProps {
  eyebrow?: string
  title: string
  description: string
  children?: ReactNode
}

export function SettingsSectionFrame({
  title,
  description,
  children,
}: SettingsSectionFrameProps) {
  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[580px] px-6 py-10">
        <h1 className="mb-10 text-[22px] font-semibold tracking-tight text-[#111111]">
          {title}
        </h1>
        {description ? (
          <p className="mb-8 -mt-6 text-[13px] leading-5 text-[#6B7280]">
            {description}
          </p>
        ) : null}
        {children ? (
          <div className="space-y-10">{children}</div>
        ) : null}
      </div>
    </div>
  )
}
