'use client'

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useRouter } from 'next/navigation'
import { User, Building2, LogOut } from 'lucide-react'
import { getAvatarGradient } from '../utils'

interface UserMenuPopoverProps {
  anchorRef: React.RefObject<HTMLButtonElement | null>
  isOpen: boolean
  onClose: () => void
  userName: string
  userEmail: string
  orgName?: string
  onLogout: () => void
}

function MenuItem({
  icon: Icon,
  label,
  sublabel,
  onClick,
  danger = false,
}: {
  icon: React.ComponentType<{ size?: number; strokeWidth?: number; className?: string }>
  label: string
  sublabel?: string
  onClick: () => void
  danger?: boolean
}) {
  return (
    <button
      onClick={onClick}
      className={[
        'flex w-full items-center gap-3 rounded-[10px] px-2.5 py-2 text-left transition-colors',
        danger
          ? 'text-[#C0402A] hover:bg-[#FFF0ED]'
          : 'text-[#1C1C1E] hover:bg-[#F4F5F8]',
      ].join(' ')}
    >
      <Icon
        size={15}
        strokeWidth={1.7}
        className={danger ? 'text-[#C0402A]' : 'text-[#7A7D87]'}
      />
      <div className="min-w-0 flex-1">
        <div className="font-inter text-[13px] font-medium leading-tight">{label}</div>
        {sublabel && (
          <div className="font-inter text-[11px] text-[#9B9691]">{sublabel}</div>
        )}
      </div>
    </button>
  )
}

export function UserMenuPopover({
  anchorRef,
  isOpen,
  onClose,
  userName,
  userEmail,
  orgName,
  onLogout,
}: UserMenuPopoverProps) {
  const router = useRouter()
  const popoverRef = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState({ left: 76, bottom: 16 })

  // Compute position when popover opens
  useLayoutEffect(() => {
    if (!isOpen || !anchorRef.current) return
    const rect = anchorRef.current.getBoundingClientRect()
    setPos({
      left: rect.right + 8,
      bottom: window.innerHeight - rect.bottom - 4,
    })
  }, [isOpen, anchorRef])

  // Close on outside click and Escape
  useEffect(() => {
    if (!isOpen) return

    function handleMouseDown(e: MouseEvent) {
      if (
        popoverRef.current &&
        !popoverRef.current.contains(e.target as Node) &&
        anchorRef.current &&
        !anchorRef.current.contains(e.target as Node)
      ) {
        onClose()
      }
    }

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }

    document.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [isOpen, onClose, anchorRef])

  if (!isOpen || typeof document === 'undefined') return null

  const navigate = (href: string) => {
    onClose()
    router.push(href)
  }

  const initial = (userName?.trim()?.charAt(0) || 'U').toUpperCase()
  const gradient = getAvatarGradient(userName || 'U')

  return createPortal(
    <div
      ref={popoverRef}
      style={{ position: 'fixed', left: pos.left, bottom: pos.bottom, minWidth: 240, zIndex: 999 }}
      className="w-[252px] overflow-hidden rounded-[14px] border border-[#E4E1DC] bg-white shadow-[0_8px_40px_rgba(0,0,0,0.13),0_2px_8px_rgba(0,0,0,0.06)]"
    >
      {/* ── Identity card ──────────────────────────────────────── */}
      <div className="flex items-center gap-3 border-b border-[#F0F1F4] px-4 py-3.5">
        <div
          className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-linear-to-br text-[13px] font-semibold text-white ${gradient}`}
        >
          {initial}
        </div>
        <div className="min-w-0">
          <div className="truncate font-inter text-[13px] font-semibold text-[#1C1C1E]">
            {userName || 'My account'}
          </div>
          <div className="truncate font-inter text-[11px] text-[#8A8D96]">{userEmail}</div>
          {orgName && (
            <div className="truncate font-inter text-[11px] text-[#B0B3BC]">{orgName}</div>
          )}
        </div>
      </div>

      {/* ── Navigation items ───────────────────────────────────── */}
      <div className="p-1.5">
        <MenuItem
          icon={User}
          label="Account settings"
          sublabel="Profile, security, notifications"
          onClick={() => navigate('/profile')}
        />
        <MenuItem
          icon={Building2}
          label="Workspace settings"
          sublabel="General, members, billing"
          onClick={() => navigate('/workspace')}
        />
      </div>

      {/* ── Danger zone ────────────────────────────────────────── */}
      <div className="border-t border-[#F0F1F4] p-1.5">
        <MenuItem
          icon={LogOut}
          label="Log out"
          onClick={() => { onClose(); onLogout() }}
          danger
        />
      </div>
    </div>,
    document.body,
  )
}
