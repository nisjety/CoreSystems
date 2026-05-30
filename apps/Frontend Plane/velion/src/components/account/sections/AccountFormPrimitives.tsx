'use client'

import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from 'react'

function cn(...classes: Array<string | false | null | undefined>) {
  return classes.filter(Boolean).join(' ')
}

export function AccountSection({
  id,
  title,
  description,
  children,
  className,
}: {
  id: string
  title: string
  description?: string
  children: ReactNode
  className?: string
}) {
  return (
    <section id={id} className={cn('scroll-mt-28', className)}>
      <h2 className="mb-5 text-[15px] font-semibold text-[#111111]">
        {title}
      </h2>
      {description ? (
        <p className="mb-5 text-[13px] leading-5 text-black/54">
          {description}
        </p>
      ) : null}
      {children}
    </section>
  )
}

export function FieldLabel({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <label className={cn('mb-1.5 block text-[12px] font-medium text-[#6B7280]', className)}>
      {children}
    </label>
  )
}

const fieldBaseClassName = [
  'block w-full rounded-[8px] border border-[#E0E0E0] bg-white',
  'px-3 text-[13px] text-[#111111] placeholder:text-[#BBBBBB]',
  'outline-none transition',
  'focus:border-[#999] focus:ring-2 focus:ring-black/5',
].join(' ')

export function AccountInput({
  className,
  readOnly,
  ...props
}: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      readOnly={readOnly}
      {...props}
      className={cn(
        fieldBaseClassName,
        'h-10',
        readOnly ? 'cursor-default text-black/45' : '',
        className,
      )}
    />
  )
}

export function AccountTextarea({
  className,
  ...props
}: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={cn(fieldBaseClassName, 'min-h-[80px] py-2.5 leading-6', className)}
    />
  )
}

export function AccountSelect({
  className,
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={cn(
        fieldBaseClassName,
        'h-10 appearance-none bg-[linear-gradient(45deg,transparent_50%,#6f6f6a_50%),linear-gradient(135deg,#6f6f6a_50%,transparent_50%)] bg-[position:calc(100%-18px)_calc(50%-2px),calc(100%-12px)_calc(50%-2px)] bg-[size:5px_5px,5px_5px] bg-no-repeat pr-10',
        className,
      )}
    >
      {children}
    </select>
  )
}

export function PrimaryButton({
  className,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      className={cn(
        'inline-flex items-center justify-center gap-2 rounded-full bg-[#171717] px-5 py-3 text-sm font-medium text-white transition hover:bg-[#2B2B2B] disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
    >
      {children}
    </button>
  )
}

export function SecondaryButton({
  className,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      className={cn(
        'inline-flex items-center justify-center gap-2 rounded-full border border-[#E4E1DC] bg-white px-5 py-3 text-sm font-medium text-black/70 transition hover:border-[#d4d1c7] hover:text-[#171717]',
        className,
      )}
    >
      {children}
    </button>
  )
}

export function InlineNotice({
  tone = 'neutral',
  children,
}: {
  tone?: 'neutral' | 'success' | 'danger'
  children: ReactNode
}) {
  const toneClassName =
    tone === 'success'
      ? 'border-[#D8ECDF] bg-[#F4FBF6] text-[#25633A]'
      : tone === 'danger'
        ? 'border-[#F2D0CB] bg-[#FFF6F4] text-[#B53F30]'
        : 'border-[#e8e3d8] bg-[#fbfaf6] text-[#6d6d67]'

  return (
    <div className={cn('rounded-[22px] border px-4 py-3 text-sm leading-6', toneClassName)}>
      {children}
    </div>
  )
}
