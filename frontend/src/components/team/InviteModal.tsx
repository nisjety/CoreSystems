'use client';

import { useState } from 'react';
import { X, Plus } from 'lucide-react';

interface InviteModalProps {
  onClose: () => void;
  onInvite: (emails: string[], role: 'admin' | 'member' | 'viewer') => Promise<void>;
}

const ROLES: Array<{ value: 'admin' | 'member' | 'viewer'; label: string; description: string }> = [
  { value: 'admin',  label: 'Admin',     description: 'Full tilgang — kan endre innstillinger og invitere.' },
  { value: 'member', label: 'Medlem',    description: 'Kan søke, chatte og se kunnskapsbasen.' },
  { value: 'viewer', label: 'Tilskuer',  description: 'Les-tilgang kun. Kan ikke redigere.' },
];

export function InviteModal({ onClose, onInvite }: InviteModalProps) {
  const [emailInput, setEmailInput] = useState('');
  const [emails, setEmails] = useState<string[]>([]);
  const [role, setRole] = useState<'admin' | 'member' | 'viewer'>('member');
  const [isSending, setIsSending] = useState(false);
  const [error, setError] = useState('');

  const addEmail = () => {
    const e = emailInput.trim().toLowerCase();
    if (!e || emails.includes(e)) { setEmailInput(''); return; }
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e)) { setError('Ugyldig e-postadresse.'); return; }
    setEmails((prev) => [...prev, e]);
    setEmailInput('');
    setError('');
  };

  const removeEmail = (e: string) => setEmails((prev) => prev.filter((x) => x !== e));

  const handleSend = async () => {
    if (emails.length === 0) { setError('Legg til minst én e-postadresse.'); return; }
    setIsSending(true);
    try {
      await onInvite(emails, role);
      onClose();
    } catch {
      setError('Kunne ikke sende invitasjon. Prøv igjen.');
    } finally {
      setIsSending(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[#2B2B2B]/30 p-4 backdrop-blur-sm">
      <div className="w-full max-w-md border border-[#D8D2C6] bg-[#F4F1EB]">

        {/* Header */}
        <div className="flex items-center justify-between border-b border-[#D8D2C6] px-6 py-4">
          <h2
            className="text-[22px] font-normal text-[#2B2B2B]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            Inviter teammedlem
          </h2>
          <button onClick={onClose} className="text-[#C8C1B3] hover:text-[#2B2B2B]">
            <X size={16} strokeWidth={1.5} />
          </button>
        </div>

        <div className="px-6 py-6 space-y-6">

          {/* Email input */}
          <div>
            <label className="mb-2 block font-inter text-[11px] uppercase tracking-widest text-[#A09890]">
              E-postadresse
            </label>
            <div className="flex gap-2">
              <input
                type="email"
                value={emailInput}
                onChange={(e) => { setEmailInput(e.target.value); setError(''); }}
                onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), addEmail())}
                placeholder="kollega@selskap.no"
                className="flex-1 border border-[#D8D2C6] bg-white px-4 py-2.5 font-inter text-[13px] text-[#2B2B2B] outline-none transition-colors placeholder:text-[#C8C1B3] focus:border-[#2B2B2B]"
              />
              <button
                type="button"
                onClick={addEmail}
                className="flex items-center gap-1 border border-[#D8D2C6] px-3 font-inter text-[11px] text-[#4A4A48] hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
              >
                <Plus size={12} strokeWidth={1.5} />
              </button>
            </div>
            {error && <p className="mt-1.5 font-inter text-[11px] text-[#FF2E63]">{error}</p>}

            {/* Email chips */}
            {emails.length > 0 && (
              <div className="mt-3 flex flex-wrap gap-1.5">
                {emails.map((e) => (
                  <span key={e} className="flex items-center gap-1.5 border border-[#D8D2C6] bg-[#EAE6DF] px-2.5 py-1 font-inter text-[11px] text-[#4A4A48]">
                    {e}
                    <button onClick={() => removeEmail(e)} className="text-[#C8C1B3] hover:text-[#2B2B2B]">
                      <X size={10} strokeWidth={2} />
                    </button>
                  </span>
                ))}
              </div>
            )}
          </div>

          {/* Role selector */}
          <div>
            <label className="mb-2 block font-inter text-[11px] uppercase tracking-widest text-[#A09890]">
              Rolle
            </label>
            <div className="space-y-1">
              {ROLES.map((r) => (
                <button
                  key={r.value}
                  type="button"
                  onClick={() => setRole(r.value)}
                  className={`w-full flex items-start gap-3 border px-4 py-3 text-left transition-colors ${
                    role === r.value
                      ? 'border-[#2B2B2B] bg-[#EAE6DF]'
                      : 'border-[#D8D2C6] bg-transparent hover:bg-[#EAE6DF]/50'
                  }`}
                >
                  <span className={`mt-0.5 h-3 w-3 shrink-0 rounded-full border ${role === r.value ? 'border-[#2B2B2B] bg-[#2B2B2B]' : 'border-[#D8D2C6]'}`} />
                  <div>
                    <p className="font-inter text-[12px] font-medium text-[#2B2B2B]">{r.label}</p>
                    <p className="font-inter text-[11px] text-[#A09890]">{r.description}</p>
                  </div>
                </button>
              ))}
            </div>
          </div>

          {/* Actions */}
          <div className="flex gap-2 pt-2">
            <button
              onClick={onClose}
              className="flex-1 border border-[#D8D2C6] py-2.5 font-inter text-[11px] uppercase tracking-widest text-[#A09890] transition-colors hover:border-[#2B2B2B] hover:text-[#2B2B2B]"
            >
              Avbryt
            </button>
            <button
              onClick={handleSend}
              disabled={isSending || emails.length === 0}
              className="flex-1 bg-[#111111] py-2.5 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {isSending ? 'Sender…' : 'Send invitasjon'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
