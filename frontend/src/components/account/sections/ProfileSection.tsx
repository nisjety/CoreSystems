'use client'

import { useState } from 'react'
import { Camera, Loader2, Check, AlertCircle } from 'lucide-react'
import Image from 'next/image'
import { getAvatarGradient } from '@/components/core/sidebar/utils'
import { useCurrentProfile, useUpdateProfile } from '../hooks/useAccount'
import type { AccountProfileFormValues } from '../types'

const TIMEZONES = [
  'UTC', 'Europe/Oslo', 'Europe/London', 'Europe/Berlin', 'Europe/Paris',
  'America/New_York', 'America/Chicago', 'America/Denver', 'America/Los_Angeles',
  'Asia/Tokyo', 'Asia/Singapore', 'Asia/Dubai', 'Australia/Sydney',
]

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <label className="block font-inter text-[11px] font-medium uppercase tracking-widest text-[#9B9691] mb-1.5">
      {children}
    </label>
  )
}

function Input({
  value,
  onChange,
  placeholder,
  type = 'text',
  readOnly,
}: {
  value: string
  onChange?: (v: string) => void
  placeholder?: string
  type?: string
  readOnly?: boolean
}) {
  return (
    <input
      type={type}
      value={value}
      readOnly={readOnly}
      onChange={e => onChange?.(e.target.value)}
      placeholder={placeholder}
      className={[
        'block w-full rounded-xl border border-[#D8D2C6] bg-white px-3.5 py-2.5',
        'font-inter text-[13px] text-[#1C1C1A] placeholder:text-[#C8C1B3]',
        'outline-none transition-colors focus:border-[#1C1C1A] focus:ring-1 focus:ring-[#1C1C1A]/10',
        readOnly ? 'cursor-default opacity-60' : '',
      ].join(' ')}
    />
  )
}

function Select({
  value,
  onChange,
  options,
}: {
  value: string
  onChange: (v: string) => void
  options: string[]
}) {
  return (
    <select
      value={value}
      onChange={e => onChange(e.target.value)}
      className="block w-full rounded-xl border border-[#D8D2C6] bg-white px-3.5 py-2.5 font-inter text-[13px] text-[#1C1C1A] outline-none transition-colors focus:border-[#1C1C1A] appearance-none"
    >
      {options.map(o => (
        <option key={o} value={o}>{o}</option>
      ))}
    </select>
  )
}

const SOCIAL_FIELDS: { key: keyof AccountProfileFormValues; label: string; placeholder: string }[] = [
  { key: 'website',  label: 'Website',  placeholder: 'https://yoursite.com' },
  { key: 'linkedIn', label: 'LinkedIn', placeholder: 'linkedin.com/in/username' },
  { key: 'github',   label: 'GitHub',   placeholder: 'github.com/username' },
  { key: 'twitter',  label: 'X / Twitter', placeholder: 'x.com/username' },
]

export function ProfileSection() {
  const { data: profile, isLoading } = useCurrentProfile()

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 py-8 text-[#9B9691] font-inter text-[13px]">
        <Loader2 size={14} className="animate-spin" /> Loading profile…
      </div>
    )
  }

  return <ProfileForm profile={profile ?? null} />
}

function ProfileForm({ profile }: { profile: { name?: string | null; email?: string; avatar?: string | null; position?: string | null; department?: string | null } | null }) {
  const updateProfile = useUpdateProfile()

  const [form, setForm] = useState<AccountProfileFormValues>({
    displayName:  profile?.name ?? '',
    bio:          '',
    location:     '',
    timezone:     'UTC',
    jobTitle:     profile?.position ?? '',
    department:   profile?.department ?? '',
    website:      '',
    linkedIn:     '',
    github:       '',
    twitter:      '',
  })
  const [avatarFailed, setAvatarFailed] = useState(false)
  const [saved, setSaved] = useState(false)

  const set = (field: keyof AccountProfileFormValues) => (v: string) =>
    setForm(prev => ({ ...prev, [field]: v }))

  const handleSave = async () => {
    await updateProfile.mutateAsync({
      name:       form.displayName,
      position:   form.jobTitle,
      department: form.department,
    })
    setSaved(true)
    setTimeout(() => setSaved(false), 2500)
  }

  const name = form.displayName || profile?.name || 'U'
  const gradient = getAvatarGradient(name)
  const avatarUrl = profile?.avatar && !avatarFailed ? profile.avatar : null

  return (
    <section id="profile" className="scroll-mt-6">
      {/* Section heading */}
      <h2 className="mb-6 font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
        Profile
      </h2>

      {/* Avatar + guidance */}
      <div className="mb-8 flex items-center gap-5">
        <div
          className={`relative h-16 w-16 shrink-0 overflow-hidden rounded-full flex items-center justify-center text-white font-semibold text-[22px] bg-linear-to-br ${gradient}`}
        >
          {avatarUrl ? (
            <Image
              src={avatarUrl}
              alt={name}
              fill
              unoptimized
              sizes="64px"
              className="object-cover"
              onError={() => setAvatarFailed(true)}
            />
          ) : null}
          <button
            className="absolute inset-0 flex items-center justify-center rounded-full bg-black/0 opacity-0 hover:bg-black/30 hover:opacity-100 transition-all"
            title="Change avatar"
          >
            <Camera size={16} className="text-white" />
          </button>
        </div>
        <p className="font-inter text-[12px] leading-relaxed text-[#9B9691] max-w-[340px]">
          Update your avatar by clicking the image. Recommended 288×288 px,
          PNG or JPG only.
        </p>
      </div>

      {/* Display name + bio */}
      <div className="grid gap-4 mb-4">
        <div>
          <SectionLabel>Display name</SectionLabel>
          <Input value={form.displayName} onChange={set('displayName')} placeholder="Your name" />
        </div>
        <div>
          <SectionLabel>Bio / Slogan</SectionLabel>
          <Input value={form.bio} onChange={set('bio')} placeholder="A short description about you" />
        </div>
      </div>

      {/* Email + Location */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 mb-4">
        <div>
          <SectionLabel>Email</SectionLabel>
          <Input value={profile?.email ?? ''} readOnly placeholder="—" />
        </div>
        <div>
          <SectionLabel>Location</SectionLabel>
          <Input value={form.location} onChange={set('location')} placeholder="City, Country" />
        </div>
      </div>

      {/* Job title + Department */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 mb-4">
        <div>
          <SectionLabel>Job title</SectionLabel>
          <Input value={form.jobTitle} onChange={set('jobTitle')} placeholder="e.g. Software Engineer" />
        </div>
        <div>
          <SectionLabel>Department</SectionLabel>
          <Input value={form.department} onChange={set('department')} placeholder="e.g. Engineering" />
        </div>
      </div>

      {/* Timezone */}
      <div className="mb-8">
        <SectionLabel>Timezone</SectionLabel>
        <Select value={form.timezone} onChange={set('timezone')} options={TIMEZONES} />
      </div>

      {/* Social profiles sub-section */}
      <h3 className="mb-4 font-inter text-[16px] font-semibold tracking-[-0.01em] text-[#1C1C1A]">
        Social profiles
      </h3>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        {SOCIAL_FIELDS.map(({ key, label, placeholder }) => (
          <div key={key}>
            <SectionLabel>{label}</SectionLabel>
            <Input value={form[key] as string} onChange={set(key)} placeholder={placeholder} />
          </div>
        ))}
      </div>

      {/* Save button */}
      <div className="mt-8 flex items-center gap-3">
        <button
          onClick={handleSave}
          disabled={updateProfile.isPending}
          className="flex items-center gap-2 rounded-xl bg-[#1C1C1A] px-5 py-2.5 font-inter text-[13px] font-medium text-white transition-colors hover:bg-[#383530] disabled:opacity-60"
        >
          {updateProfile.isPending ? (
            <><Loader2 size={14} className="animate-spin" /> Saving…</>
          ) : saved ? (
            <><Check size={14} /> Saved</>
          ) : (
            'Save changes'
          )}
        </button>
        {updateProfile.isError && (
          <span className="flex items-center gap-1.5 font-inter text-[12px] text-[#C0402A]">
            <AlertCircle size={13} /> Failed to save — please try again.
          </span>
        )}
      </div>
    </section>
  )
}
