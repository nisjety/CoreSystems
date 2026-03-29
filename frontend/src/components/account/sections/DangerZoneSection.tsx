'use client'

import { useState } from 'react'
import { Trash2, AlertTriangle, Loader2 } from 'lucide-react'
import { useRouter } from 'next/navigation'
import { useDeleteAccount } from '../hooks/useAccount'

export function DangerZoneSection() {
  const router = useRouter()
  const deleteAccount = useDeleteAccount()
  const [confirmText, setConfirmText] = useState('')
  const [showConfirm, setShowConfirm] = useState(false)

  const CONFIRM_PHRASE = 'delete my account'
  const isConfirmed = confirmText.trim().toLowerCase() === CONFIRM_PHRASE

  const handleDelete = async () => {
    if (!isConfirmed) return
    await deleteAccount.mutateAsync()
    router.push('/auth/sign-in?deleted=1')
  }

  return (
    <section id="danger" className="scroll-mt-6">
      <h2 className="mb-1.5 font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
        Delete account
      </h2>
      <p className="mb-6 font-inter text-[13px] text-[#9B9691]">
        Permanently remove your account and all associated data. This action cannot be undone.
      </p>

      <div className="rounded-[10px] border border-[#F5C9C4] bg-[#FFF8F7] p-5">
        <div className="mb-4 flex items-start gap-3">
          <AlertTriangle size={16} className="mt-0.5 shrink-0 text-[#C0402A]" />
          <div>
            <p className="font-inter text-[13px] font-medium text-[#C0402A]">
              This is a destructive action
            </p>
            <p className="mt-0.5 font-inter text-[12px] text-[#9B8482]">
              Your profile, all messages, uploaded files, and organisation memberships
              will be permanently deleted. There is no recovery option.
            </p>
          </div>
        </div>

        {!showConfirm ? (
          <button
            onClick={() => setShowConfirm(true)}
            className="flex items-center gap-2 rounded-xl border border-[#F5C9C4] bg-white px-4 py-2.5 font-inter text-[13px] font-medium text-[#C0402A] transition-colors hover:bg-[#FFF0ED]"
          >
            <Trash2 size={13} />
            Delete my account
          </button>
        ) : (
          <div className="space-y-3">
            <p className="font-inter text-[12px] text-[#9B8482]">
              To confirm, type{' '}
              <strong className="font-mono font-semibold text-[#1C1C1A]">
                delete my account
              </strong>{' '}
              below:
            </p>
            <input
              type="text"
              value={confirmText}
              onChange={e => setConfirmText(e.target.value)}
              placeholder="delete my account"
              className="block w-full rounded-xl border border-[#F5C9C4] bg-white px-3.5 py-2.5 font-inter text-[13px] text-[#1C1C1A] placeholder:text-[#D0C9BF] outline-none transition-colors focus:border-[#C0402A] focus:ring-1 focus:ring-[#C0402A]/10"
            />
            <div className="flex items-center gap-2">
              <button
                onClick={handleDelete}
                disabled={!isConfirmed || deleteAccount.isPending}
                className="flex items-center gap-2 rounded-xl bg-[#C0402A] px-4 py-2.5 font-inter text-[13px] font-medium text-white transition-colors hover:bg-[#A5361F] disabled:cursor-not-allowed disabled:opacity-40"
              >
                {deleteAccount.isPending ? (
                  <><Loader2 size={13} className="animate-spin" /> Deleting…</>
                ) : (
                  <><Trash2 size={13} /> Confirm deletion</>
                )}
              </button>
              <button
                onClick={() => { setShowConfirm(false); setConfirmText('') }}
                className="rounded-xl px-4 py-2.5 font-inter text-[13px] text-[#9B9691] hover:text-[#1C1C1A]"
              >
                Cancel
              </button>
            </div>
            {deleteAccount.isError && (
              <p className="font-inter text-[12px] text-[#C0402A]">
                {deleteAccount.error instanceof Error
                  ? deleteAccount.error.message
                  : 'Account deletion failed. Please contact support.'}
              </p>
            )}
          </div>
        )}
      </div>
    </section>
  )
}
