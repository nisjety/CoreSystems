'use client';

type Role = 'owner' | 'admin' | 'member' | 'viewer';

const ROLE_CONFIG: Record<Role, { label: string; border: string; text: string; bg: string }> = {
  owner:  { label: 'Eier',        border: '#111111', text: '#111111',   bg: '#F4F1EB' },
  admin:  { label: 'Admin',       border: '#2B2B2B', text: '#2B2B2B',   bg: '#EAE6DF' },
  member: { label: 'Medlem',      border: '#D8D2C6', text: '#4A4A48',   bg: 'transparent' },
  viewer: { label: 'Tilskuer',    border: '#D8D2C6', text: '#A09890',   bg: 'transparent' },
};

interface RoleBadgeProps {
  role: Role;
}

export function RoleBadge({ role }: RoleBadgeProps) {
  const cfg = ROLE_CONFIG[role] ?? ROLE_CONFIG.member;
  return (
    <span
      className="inline-block border px-2 py-0.5 font-inter text-[10px] uppercase tracking-widest"
      style={{ borderColor: cfg.border, color: cfg.text, background: cfg.bg }}
    >
      {cfg.label}
    </span>
  );
}
