'use client'

import { useState } from 'react'
import { Trash2, AlertTriangle, Loader2 } from 'lucide-react'
import { useRouter } from 'next/navigation'
import { useDeleteAccount } from '../hooks/useAccount'
import {
  AccountInput,
  AccountSection,
  InlineNotice,
  PrimaryButton,
  SecondaryButton,
} from './AccountFormPrimitives'

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
    router.push('/login?deleted=1')
  }

  return (
    <AccountSection
      id="danger"
      title="Delete account"
      description=""
    >
      <div className="rounded-[28px] border border-[#F1D2CC] bg-[#fff8f6] p-6">
        <div className="mb-5 flex items-start gap-4">
          <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-white text-[#C0402A] shadow-[0_10px_22px_rgba(181,63,48,0.12)]">
            <AlertTriangle size={18} />
          </div>
          <div>
            <p className="text-[16px] font-semibold tracking-[-0.02em] text-[#AF3E31]">
              This is a destructive action
            </p>
            <p className="mt-2 text-[14px] leading-7 text-[#8F6760]">
              Your profile, all messages, uploaded files, and organisation memberships
              will be permanently deleted. There is no recovery option.
            </p>
          </div>
        </div>

        {!showConfirm ? (
          <PrimaryButton
            onClick={() => setShowConfirm(true)}
            className="bg-[#B53F30] hover:bg-[#9D3527]"
          >
            <Trash2 size={14} />
            Delete my account
          </PrimaryButton>
        ) : (
          <div className="space-y-3">
            <p className="text-[13px] leading-6 text-[#8F6760]">
              To confirm, type{' '}
              <strong className="font-mono font-semibold text-[#1C1C1A]">
                delete my account
              </strong>{' '}
              below:
            </p>
            <AccountInput
              type="text"
              value={confirmText}
              onChange={e => setConfirmText(e.target.value)}
              placeholder="delete my account"
              className="border-[#EFC8C2] bg-white focus:border-[#B53F30] focus:ring-[#B53F30]/10"
            />
            <div className="flex flex-wrap items-center gap-3">
              <PrimaryButton
                onClick={handleDelete}
                disabled={!isConfirmed || deleteAccount.isPending}
                className="bg-[#B53F30] hover:bg-[#9D3527] disabled:opacity-40"
              >
                {deleteAccount.isPending ? (
                  <><Loader2 size={14} className="animate-spin" /> Deleting…</>
                ) : (
                  <><Trash2 size={14} /> Confirm deletion</>
                )}
              </PrimaryButton>
              <SecondaryButton
                onClick={() => { setShowConfirm(false); setConfirmText('') }}
              >
                Cancel
              </SecondaryButton>
            </div>
            {deleteAccount.isError && (
              <InlineNotice tone="danger">
                {deleteAccount.error instanceof Error
                  ? deleteAccount.error.message
                  : 'Account deletion failed. Please contact support.'}
              </InlineNotice>
            )}
          </div>
        )}
      </div>
    </AccountSection>
  )
}
