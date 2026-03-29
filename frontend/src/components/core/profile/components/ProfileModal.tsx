'use client';

import React, { useState } from 'react';
import Image from 'next/image';
import { createPortal } from 'react-dom';
import { useRouter } from 'next/navigation';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  X,
  Camera,
  Check,
  Loader2,
  Mail,
  Briefcase,
  Building2,
  Link2,
  Shield,
  Github,
  Chrome,
  Circle,
  Clock,
  MinusCircle,
  Users,
  Crown,
  BadgeCheck,
  ExternalLink,
  LogOut,
} from 'lucide-react';
import { useCurrentProfile, useLinkedProviders, useUpdateProfile, useUserOrganizations, useCurrentOrganization } from '../hooks/useProfile';
import { authORPCClient } from '@/components/auth/lib/orpc/client';
import { getAvatarGradient } from '../../sidebar/utils';
import type { ProviderAccount, UserProfile } from '../types';
import type { Organization } from '@/lib/services';

interface ProfileModalProps {
  onClose: () => void;
}

// ─── Provider display registry ──────────────────────────────────────────────
const PROVIDER_META: Record<string, { label: string; color: string; initial: string }> = {
  email: { label: 'Email / Password', color: '#6B7280', initial: '@' },
  microsoft: { label: 'Microsoft', color: '#00A4EF', initial: 'M' },
  google: { label: 'Google', color: '#4285F4', initial: 'G' },
  github: { label: 'GitHub', color: '#24292F', initial: '' },
  vipps: { label: 'Vipps', color: '#FF5B24', initial: 'V' },
  apple: { label: 'Apple', color: '#000000', initial: '' },
  okta: { label: 'Okta', color: '#007DC1', initial: 'O' },
};

function ProviderIcon({ provider }: { provider: string }) {
  if (provider === 'github') return <Github className="w-4 h-4" />;
  if (provider === 'google') return <Chrome className="w-4 h-4" style={{ color: '#4285F4' }} />;
  const meta = PROVIDER_META[provider];
  if (meta?.initial) {
    return (
      <span className="w-4 h-4 flex items-center justify-center font-bold text-[13px]" style={{ color: meta.color }}>
        {meta.initial}
      </span>
    );
  }
  return <Link2 className="w-4 h-4" />;
}

function ProviderBadge({ provider: p }: { provider: ProviderAccount }) {
  const meta = PROVIDER_META[p.provider] ?? { label: p.provider, color: '#9CA3AF', initial: '?' };
  return (
    <div className="flex items-center gap-3 px-4 py-3 rounded-xl bg-white border border-black/10 hover:border-black/20 transition-colors">
      <div className="w-8 h-8 rounded-lg flex items-center justify-center bg-black/5 shrink-0">
        <ProviderIcon provider={p.provider} />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-semibold text-black">{meta.label}</p>
        {p.email && <p className="text-xs text-black/45 truncate">{p.email}</p>}
      </div>
      <Shield className="w-4 h-4 text-green-500 shrink-0" />
    </div>
  );
}

// ─── Status selector ─────────────────────────────────────────────────────────
const STATUSES = [
  { value: 'online', label: 'Online', icon: <Circle className="w-3 h-3 fill-green-500 text-green-500" /> },
  { value: 'away', label: 'Away', icon: <Clock className="w-3 h-3 text-yellow-500" /> },
  { value: 'busy', label: 'Busy', icon: <MinusCircle className="w-3 h-3 text-red-500" /> },
  { value: 'offline', label: 'Offline', icon: <Circle className="w-3 h-3 fill-gray-400 text-gray-400" /> },
] as const;

// ─── Avatar ───────────────────────────────────────────────────────────────────
function ProfileAvatar({ profile, size = 64 }: { profile?: UserProfile | null; size?: number }) {
  const name = profile?.name ?? 'U';
  const gradient = getAvatarGradient(name);
  const [avatarLoadFailed, setAvatarLoadFailed] = useState(false);

  return (
    <div
      className={`relative rounded-full overflow-hidden flex items-center justify-center text-white font-semibold shadow-md bg-linear-to-br ${gradient} shrink-0`}
      style={{ width: size, height: size, fontSize: size * 0.35 }}
    >
      {profile?.avatar && !avatarLoadFailed ? (
        <Image
          src={profile.avatar}
          alt={name}
          fill
          unoptimized
          sizes={`${size}px`}
          className="object-cover"
          onError={() => setAvatarLoadFailed(true)}
        />
      ) : (
        name.charAt(0).toUpperCase()
      )}
    </div>
  );
}

// ─── Organization card ────────────────────────────────────────────────────────
function OrganizationCard({ org, isCurrent = false }: { org: Organization; isCurrent?: boolean }) {
  const getPlanColor = (plan: string) => {
    switch (plan) {
      case 'enterprise': return 'text-purple-600 bg-purple-50 border-purple-200';
      case 'pro': return 'text-blue-600 bg-blue-50 border-blue-200';
      default: return 'text-gray-600 bg-gray-50 border-gray-200';
    }
  };

  return (
    <div className={`flex items-start gap-4 p-4 rounded-xl border transition-colors ${
      isCurrent ? 'bg-blue-50/50 border-blue-200' : 'bg-white border-black/10 hover:border-black/20'
    }`}>
      <div className="w-12 h-12 rounded-xl flex items-center justify-center bg-linear-to-br from-blue-500 to-purple-600 text-white font-bold text-lg shrink-0 shadow-md">
        {org.name.charAt(0).toUpperCase()}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <p className="text-sm font-semibold text-black truncate">{org.name}</p>
          {isCurrent && (
            <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-blue-100 text-blue-700 text-[10px] font-medium">
              <Circle className="w-2 h-2 fill-current" />
              Active
            </span>
          )}
          {org.verificationStatus === 'verified' && (
            <span title="Verified organization">
              <BadgeCheck className="w-4 h-4 text-green-500" />
            </span>
          )}
        </div>
        {org.slug && (
          <p className="text-xs text-black/40 mb-2">@{org.slug}</p>
        )}
        <div className="flex items-center gap-2 flex-wrap">
          <span className={`inline-flex items-center px-2 py-1 rounded-md border text-[10px] font-medium uppercase tracking-wider ${getPlanColor(org.plan)}`}>
            {org.plan}
          </span>
          {org.orgNumber && (
            <span className="text-xs text-black/50">
              Org #{org.orgNumber}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Main modal content ───────────────────────────────────────────────────────
function ProfileModalContent({ onClose }: ProfileModalProps) {
  const router = useRouter();
  const { data: profile, isLoading } = useCurrentProfile();
  const { data: providersData } = useLinkedProviders();
  const { data: organizations = [], isLoading: isLoadingOrgs } = useUserOrganizations();
  const { data: currentOrg } = useCurrentOrganization();
  const updateProfile = useUpdateProfile();
  const queryClient = useQueryClient();
  const signOut = useMutation({
    mutationFn: () => authORPCClient.signOut(),
    onSuccess: () => {
      queryClient.clear();
      window.location.href = '/sign-in';
    },
  });

  const [activeTab, setActiveTab] = useState<'profile' | 'connections' | 'organizations'>('profile');
  const [name, setName] = useState('');
  const [position, setPosition] = useState('');
  const [department, setDepartment] = useState('');
  const [status, setStatus] = useState<UserProfile['status']>('online');
  const [justSaved, setJustSaved] = useState(false);
  const [seededForId, setSeededForId] = useState<string | undefined>(undefined);

  const providers = Array.isArray(providersData)
    ? providersData
    : (providersData && typeof providersData === 'object' && Array.isArray((providersData as { providers?: ProviderAccount[] }).providers))
      ? (providersData as { providers: ProviderAccount[] }).providers
      : [];

  // Seed form when profile first loads — React's recommended "adjust state during render"
  // pattern to avoid useEffect → setState cascade (react.dev/learn/you-might-not-need-an-effect)
  if (profile && profile.id !== seededForId) {
    setSeededForId(profile.id);
    setName(profile.name ?? '');
    setPosition(profile.position ?? '');
    setDepartment(profile.department ?? '');
    setStatus(profile.status ?? 'online');
  }

  const isDirty =
    profile != null &&
    (name !== (profile.name ?? '') ||
      position !== (profile.position ?? '') ||
      department !== (profile.department ?? '') ||
      status !== profile.status);

  const handleSave = async () => {
    if (!isDirty) return;
    await updateProfile.mutateAsync({ name, position, department, status });
    setJustSaved(true);
    setTimeout(() => setJustSaved(false), 2500);
  };

  return (
    <div
      className="fixed inset-0 z-220 flex items-center justify-center"
      style={{ background: 'rgba(0,0,0,0.45)', backdropFilter: 'blur(4px)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="relative w-[560px] max-w-[95vw] max-h-[90vh] bg-white rounded-2xl shadow-2xl overflow-hidden flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-black/10">
          <h2 className="text-base font-semibold text-black">Profile</h2>
          <button
            onClick={onClose}
            className="w-8 h-8 flex items-center justify-center rounded-lg hover:bg-black/5 transition-colors"
            aria-label="Close profile"
          >
            <X className="w-4 h-4 text-black/60" />
          </button>
        </div>

        {/* Identity stripe */}
        <div className="px-6 py-5 flex items-center gap-4 bg-[#F7F8FA] border-b border-black/10">
          {isLoading ? (
            <div className="w-16 h-16 rounded-full bg-black/10 animate-pulse shrink-0" />
          ) : (
            <div className="relative shrink-0">
              <ProfileAvatar profile={profile} size={64} />
              <button
                className="absolute -bottom-1 -right-1 w-7 h-7 rounded-full bg-white border border-black/10 shadow flex items-center justify-center hover:bg-black/5 transition-colors"
                title="Change avatar (coming soon)"
                aria-label="Change avatar"
              >
                <Camera className="w-3.5 h-3.5 text-black/50" />
              </button>
            </div>
          )}
          <div className="flex-1 min-w-0">
            {isLoading ? (
              <div className="space-y-2">
                <div className="h-5 w-32 bg-black/10 rounded animate-pulse" />
                <div className="h-4 w-48 bg-black/7 rounded animate-pulse" />
              </div>
            ) : (
              <>
                <p className="text-base font-semibold text-black truncate">{profile?.name ?? '—'}</p>
                <p className="text-sm text-black/50 truncate">{profile?.email ?? '—'}</p>
                {profile?.position && (
                  <p className="text-xs text-black/40 truncate mt-0.5">{profile.position}</p>
                )}
                <div className="flex items-center gap-2 mt-1 flex-wrap">
                  {profile?.emailVerified && (
                    <span className="inline-flex items-center gap-1 text-xs text-green-600 font-medium">
                      <Check className="w-3 h-3" /> Email verified
                    </span>
                  )}
                  {currentOrg && (
                    <span className="inline-flex items-center gap-1 text-xs text-blue-600 font-medium">
                      <Building2 className="w-3 h-3" /> {currentOrg.name}
                    </span>
                  )}
                </div>
              </>
            )}
          </div>
          {/* Quick actions */}
          <div className="flex flex-col gap-1.5 shrink-0">
            <button
              onClick={() => { onClose(); router.push('/profile'); }}
              className="flex items-center gap-1.5 h-8 px-3 rounded-lg border border-black/15 bg-white text-xs font-medium text-black/70 hover:bg-black/5 hover:text-black transition-colors"
            >
              <ExternalLink className="w-3.5 h-3.5" />
              Edit profile
            </button>
            <button
              onClick={() => signOut.mutate()}
              disabled={signOut.isPending}
              className="flex items-center gap-1.5 h-8 px-3 rounded-lg border border-red-100 bg-red-50/60 text-xs font-medium text-red-600 hover:bg-red-100 hover:border-red-200 transition-colors disabled:opacity-50"
            >
              {signOut.isPending
                ? <Loader2 className="w-3.5 h-3.5 animate-spin" />
                : <LogOut className="w-3.5 h-3.5" />}
              Sign out
            </button>
          </div>
        </div>

        {/* Tabs */}
        <div className="flex border-b border-black/10 px-6 bg-white">
          {(['profile', 'organizations', 'connections'] as const).map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`px-4 py-3 text-sm font-medium capitalize transition-colors border-b-2 -mb-px ${
                activeTab === tab
                  ? 'border-black text-black'
                  : 'border-transparent text-black/45 hover:text-black/70'
              }`}
            >
              {tab}
              {tab === 'connections' && providers.length > 0 && (
                <span className="ml-1.5 inline-flex items-center justify-center w-4 h-4 rounded-full bg-black/10 text-black/60 text-[10px] font-semibold">
                  {providers.length}
                </span>
              )}
              {tab === 'organizations' && organizations.length > 0 && (
                <span className="ml-1.5 inline-flex items-center justify-center w-4 h-4 rounded-full bg-black/10 text-black/60 text-[10px] font-semibold">
                  {organizations.length}
                </span>
              )}
            </button>
          ))}
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto p-6">
          {/* ── Profile tab ── */}
          {activeTab === 'profile' && (
            <div className="space-y-5">
              {/* Display name */}
              <div>
                <label className="block text-xs font-semibold text-black/50 uppercase tracking-wider mb-1.5">
                  Display Name
                </label>
                <input
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="Your full name"
                  className="w-full h-10 px-3 rounded-xl border border-black/15 bg-white text-sm text-black placeholder-black/30 focus:outline-none focus:ring-2 focus:ring-black/10 transition-all"
                />
              </div>

              {/* Email (read-only) */}
              <div>
                <label className="block text-xs font-semibold text-black/50 uppercase tracking-wider mb-1.5">
                  Email
                </label>
                <div className="flex items-center gap-2 h-10 px-3 rounded-xl border border-black/10 bg-black/3 text-sm text-black/55 cursor-not-allowed select-none">
                  <Mail className="w-4 h-4 shrink-0 text-black/30" />
                  <span className="truncate">{profile?.email ?? '—'}</span>
                  <span className="ml-auto text-xs text-black/30 shrink-0">read-only</span>
                </div>
              </div>

              {/* Position */}
              <div>
                <label className="block text-xs font-semibold text-black/50 uppercase tracking-wider mb-1.5">
                  Position / Title
                </label>
                <div className="relative">
                  <Briefcase className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-black/30" />
                  <input
                    type="text"
                    value={position}
                    onChange={(e) => setPosition(e.target.value)}
                    placeholder="e.g. Senior Engineer"
                    className="w-full h-10 pl-9 pr-3 rounded-xl border border-black/15 bg-white text-sm text-black placeholder-black/30 focus:outline-none focus:ring-2 focus:ring-black/10 transition-all"
                  />
                </div>
              </div>

              {/* Department */}
              <div>
                <label className="block text-xs font-semibold text-black/50 uppercase tracking-wider mb-1.5">
                  Department
                </label>
                <div className="relative">
                  <Building2 className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-black/30" />
                  <input
                    type="text"
                    value={department}
                    onChange={(e) => setDepartment(e.target.value)}
                    placeholder="e.g. Engineering"
                    className="w-full h-10 pl-9 pr-3 rounded-xl border border-black/15 bg-white text-sm text-black placeholder-black/30 focus:outline-none focus:ring-2 focus:ring-black/10 transition-all"
                  />
                </div>
              </div>

              {/* Status */}
              <div>
                <label className="block text-xs font-semibold text-black/50 uppercase tracking-wider mb-2">
                  Status
                </label>
                <div className="grid grid-cols-2 gap-2">
                  {STATUSES.map((s) => (
                    <button
                      key={s.value}
                      onClick={() => setStatus(s.value)}
                      className={`flex items-center gap-2.5 h-10 px-3 rounded-xl border text-sm font-medium transition-all ${
                        status === s.value
                          ? 'border-black/20 bg-black/5 text-black'
                          : 'border-black/10 bg-white text-black/55 hover:border-black/15 hover:bg-black/3'
                      }`}
                    >
                      {s.icon}
                      {s.label}
                      {status === s.value && <Check className="w-3 h-3 ml-auto text-black/60" />}
                    </button>
                  ))}
                </div>
              </div>

            </div>
          )}

          {/* ── Organizations tab ── */}
          {activeTab === 'organizations' && (
            <div>
              <p className="text-sm text-black/50 mb-4">
                Organizations you are a member of. Your current active organization is highlighted.
              </p>
              {isLoadingOrgs ? (
                <div className="space-y-3">
                  {[1, 2].map((i) => (
                    <div key={i} className="h-20 rounded-xl bg-black/5 animate-pulse" />
                  ))}
                </div>
              ) : organizations.length === 0 ? (
                <div className="flex flex-col items-center gap-3 py-10 text-black/30">
                  <Building2 className="w-8 h-8" />
                  <p className="text-sm">No organizations found</p>
                  <p className="text-xs text-center max-w-xs">
                    You are not a member of any organization yet. Create or join an organization to get started.
                  </p>
                </div>
              ) : (
                <div className="space-y-3">
                  {organizations.map((org) => (
                    <OrganizationCard
                      key={org.id}
                      org={org}
                      isCurrent={currentOrg?.id === org.id}
                    />
                  ))}
                </div>
              )}
            </div>
          )}

          {/* ── Connections tab ── */}
          {activeTab === 'connections' && (
            <div>
              <p className="text-sm text-black/50 mb-4">
                Sign-in methods linked to your account. Multiple providers with the same email
                address share one identity.
              </p>
              {providers.length === 0 ? (
                <div className="flex flex-col items-center gap-3 py-10 text-black/30">
                  <Link2 className="w-8 h-8" />
                  <p className="text-sm">No linked providers found</p>
                  <p className="text-xs text-center max-w-xs">
                    Sign in with Microsoft, Google, or another provider using the same email to
                    automatically link accounts
                  </p>
                </div>
              ) : (
                <div className="space-y-2">
                  {providers.map((p) => (
                    <ProviderBadge key={p.id} provider={p} />
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Footer — only on profile tab */}
        {activeTab === 'profile' && (
          <div className="px-6 py-4 border-t border-black/10 bg-[#F7F8FA] flex items-center justify-between">
            <p className="text-xs text-black/35">Changes are saved to your user-service profile</p>
            <button
              onClick={handleSave}
              disabled={!isDirty || updateProfile.isPending}
              className={`h-9 px-5 rounded-xl text-sm font-semibold transition-all inline-flex items-center gap-2 ${
                isDirty && !updateProfile.isPending
                  ? 'bg-black text-white hover:bg-black/85 active:scale-95'
                  : 'bg-black/10 text-black/30 cursor-not-allowed'
              }`}
            >
              {updateProfile.isPending ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  Saving…
                </>
              ) : justSaved ? (
                <>
                  <Check className="w-4 h-4" />
                  Saved
                </>
              ) : (
                'Save changes'
              )}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Portal wrapper ───────────────────────────────────────────────────────────
export function ProfileModal({ onClose }: ProfileModalProps) {
  if (typeof window === 'undefined') return null;
  return createPortal(<ProfileModalContent onClose={onClose} />, document.body);
}
