'use client'

import React from 'react'

import { PageFooterLinks } from '../AuthPageParts'

interface AuthShellPlaceholderProps {
  title?: string
  description?: string
  tone?: 'loading' | 'error'
}

export function AuthShellPlaceholder({
  title,
  description,
  tone = 'loading',
}: AuthShellPlaceholderProps) {
  const isError = tone === 'error'

  return (
    <div
      className="relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 sm:px-4 md:px-5 lg:px-6 xl:px-10"
      style={
        {
          '--primary': '#111111',
          '--primary-foreground': '#ffffff',
          '--ring': '#111111',
        } as React.CSSProperties
      }
    >
      <div className="relative z-[120] grid w-full max-w-[70.5rem] overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr] xl:max-w-[72rem]">
        <div className="flex min-h-[560px] items-center justify-center rounded-l-[24px] bg-white px-5 py-6 sm:px-7 sm:py-7 md:px-8 md:py-8 lg:min-h-[600px] lg:px-10 lg:py-9 xl:min-h-[640px] xl:px-16 xl:py-10">
          <div className="flex max-w-[21rem] flex-col items-center text-center">
            {isError ? (
              <span
                aria-hidden="true"
                className="flex h-8 w-8 items-center justify-center rounded-full border border-[#FCA5A5] bg-[#FEF2F2] font-inter text-[16px] text-[#B91C1C]"
              >
                !
              </span>
            ) : (
              <span
                aria-hidden="true"
                className="block h-5 w-5 animate-spin rounded-full border-2 border-[#D6D2CB] border-t-[#1F1B17]"
              />
            )}

            {(title || description) && (
              <div className="mt-5">
                {title && (
                  <p
                    className="text-[20px] font-normal leading-[1.05] tracking-normal text-[#1F1B17]"
                    style={{ fontFamily: 'var(--font-geist-sans), Arial, sans-serif' }}
                  >
                    {title}
                  </p>
                )}
                {description && (
                  <p className="mt-2 font-inter text-[12px] leading-5 text-[#6B6660]">
                    {description}
                  </p>
                )}
              </div>
            )}
          </div>
        </div>

        <div
          className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] bg-cover bg-center md:block lg:min-h-[600px] xl:min-h-[640px]"
          style={{
            backgroundImage: "url('/imagens/curved-interior-sculpture.png')",
          }}
        />
      </div>

      <PageFooterLinks
        imprint="Om oss"
        privacy="Personvern"
        copyright="Opphavsrett"
        cookieSettings="Cookie-innstillinger"
        onCookieSettings={() => undefined}
      />
    </div>
  )
}
