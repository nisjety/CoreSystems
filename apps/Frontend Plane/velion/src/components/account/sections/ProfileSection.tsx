'use client'

import { useState } from 'react'
import { Camera, Loader2, Check, AlertCircle } from 'lucide-react'
import Image from 'next/image'
import { getAvatarGradient } from '@/components/core/sidebar/utils'
import { useCurrentProfile, useUpdateProfile } from '../hooks/useAccount'
import type { AccountProfileFormValues } from '../types'
import {
  AccountInput,
  AccountSection,
  AccountSelect,
  FieldLabel,
  InlineNotice,
  PrimaryButton,
} from './AccountFormPrimitives'

const TIMEZONES = [
  'UTC', 'Europe/Oslo', 'Europe/London', 'Europe/Berlin', 'Europe/Paris',
  'America/New_York', 'America/Chicago', 'America/Denver', 'America/Los_Angeles',
  'Asia/Tokyo', 'Asia/Singapore', 'Asia/Dubai', 'Australia/Sydney',
]

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
      <div className="flex items-center gap-2 py-8 text-sm text-black/54">
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
    <AccountSection
      id="profile"
      title="Profile"
      description=""
    >
      <div className="space-y-8">
        <div className="flex flex-col gap-5 sm:flex-row sm:items-center">
          <div
            className={`relative flex h-24 w-24 shrink-0 items-center justify-center overflow-hidden rounded-full bg-linear-to-br text-[28px] font-semibold text-white ${gradient}`}
          >
            {avatarUrl ? (
              <Image
                src={avatarUrl}
                alt={name}
                fill
                unoptimized
                sizes="96px"
                className="object-cover"
                onError={() => setAvatarFailed(true)}
              />
            ) : null}
            <button
              className="absolute inset-0 flex items-center justify-center rounded-full bg-black/0 opacity-0 transition-all hover:bg-black/35 hover:opacity-100"
              title="Change avatar"
              type="button"
            >
              <Camera size={18} className="text-white" />
            </button>
          </div>
          <div className="max-w-[460px]">
            <p className="text-sm leading-6 text-black/54">
              Update your avatar by clicking the image 288x288 px size recommended in PNG or JPG format only.
            </p>
          </div>
        </div>

        <div className="grid gap-5">
          <div>
            <FieldLabel>Display name</FieldLabel>
            <AccountInput
              value={form.displayName}
              onChange={e => set('displayName')(e.target.value)}
              placeholder="Your name"
            />
          </div>
          <div>
            <FieldLabel>Slogan</FieldLabel>
            <AccountInput
              value={form.bio}
              onChange={e => set('bio')(e.target.value)}
              placeholder="i.e. Daily curated premium assets for startups and creators."
            />
          </div>
        </div>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          <div>
            <FieldLabel>Email</FieldLabel>
            <AccountInput value={profile?.email ?? ''} readOnly placeholder="—" />
          </div>
          <div>
            <FieldLabel>Location</FieldLabel>
            <AccountSelect value={form.location} onChange={e => set('location')(e.target.value)}>
              <option value="">Select location</option>
              <option value="Oslo, Norway">Oslo, Norway</option>
              <option value="London, UK">London, UK</option>
              <option value="New York, USA">New York, USA</option>
              <option value="Remote">Remote</option>
            </AccountSelect>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          <div>
            <FieldLabel>Job title</FieldLabel>
            <AccountInput
              value={form.jobTitle}
              onChange={e => set('jobTitle')(e.target.value)}
              placeholder="e.g. Product designer"
            />
          </div>
          <div>
            <FieldLabel>Department</FieldLabel>
            <AccountInput
              value={form.department}
              onChange={e => set('department')(e.target.value)}
              placeholder="e.g. Operations"
            />
          </div>
        </div>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          <div>
            <FieldLabel>Timezone</FieldLabel>
            <AccountSelect
              value={form.timezone}
              onChange={e => set('timezone')(e.target.value)}
            >
              {TIMEZONES.map(option => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </AccountSelect>
          </div>
          <div />
        </div>

        <div>
          <div className="mb-5">
            <h3 className="text-[15px] font-semibold text-[#111111]">
              Social profiles
            </h3>
          </div>
          <div className="grid grid-cols-1 gap-5 sm:grid-cols-2">
            {SOCIAL_FIELDS.map(({ key, label, placeholder }) => (
              <div key={key}>
                <FieldLabel>{label}</FieldLabel>
                <AccountInput
                  value={form[key] as string}
                  onChange={e => set(key)(e.target.value)}
                  placeholder={placeholder}
                />
              </div>
            ))}
          </div>
        </div>

        <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
          <PrimaryButton onClick={handleSave} disabled={updateProfile.isPending}>
            {updateProfile.isPending ? (
              <>
                <Loader2 size={15} className="animate-spin" />
                Saving changes
              </>
            ) : saved ? (
              <>
                <Check size={15} />
                Saved
              </>
            ) : (
              'Save changes'
            )}
          </PrimaryButton>

          {updateProfile.isError ? (
            <InlineNotice tone="danger">
              <span className="inline-flex items-center gap-2">
                <AlertCircle size={14} />
                Failed to save. Please try again.
              </span>
            </InlineNotice>
          ) : null}
        </div>
      </div>
    </AccountSection>
  )
}
