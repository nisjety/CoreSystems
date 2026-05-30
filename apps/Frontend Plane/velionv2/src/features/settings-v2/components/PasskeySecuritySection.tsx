"use client"

import { useEffect, useState } from "react"
import { Check } from "lucide-react"
import { authClient } from "@/lib/auth/auth-client"
import { VelionButton, VelionInput } from "@/components/ui/velion-ui"

type Passkey = { id: string; name?: string | null; createdAt?: string | Date | null }

type Status =
  | { type: "idle" }
  | { type: "working"; message: string }
  | { type: "error"; message: string }
  | { type: "success"; message: string }

function formatDate(value?: string | Date | null): string {
  if (!value) return ""
  const date = typeof value === "string" ? new Date(value) : value
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleDateString()
}

/**
 * Personal passkey (WebAuthn) management. Register / list / remove credentials.
 * Calls go through the same-origin Better Auth client -> /api/auth proxy ->
 * auth-core's passkey plugin (or the standalone instance in local dev).
 */
export function PasskeySecuritySection() {
  const [name, setName] = useState("")
  const [status, setStatus] = useState<Status>({ type: "idle" })
  const [passkeys, setPasskeys] = useState<Passkey[] | null>(null)

  async function refresh() {
    try {
      const result = await authClient.passkey.listUserPasskeys()
      setPasskeys((result?.data ?? []) as Passkey[])
    } catch {
      setPasskeys([])
    }
  }

  useEffect(() => {
    let active = true
    void (async () => {
      try {
        const result = await authClient.passkey.listUserPasskeys()
        if (active) setPasskeys((result?.data ?? []) as Passkey[])
      } catch {
        if (active) setPasskeys([])
      }
    })()
    return () => {
      active = false
    }
  }, [])

  async function register() {
    setStatus({ type: "working", message: "Follow the prompt from your device…" })
    try {
      const result = await authClient.passkey.addPasskey({
        name: name.trim() || undefined,
      })
      if (result && typeof result === "object" && "error" in result && result.error) {
        const message =
          (result.error as { message?: string }).message ??
          "Could not register passkey."
        setStatus({ type: "error", message })
        return
      }
      setName("")
      setStatus({ type: "success", message: "Passkey registered." })
      await refresh()
    } catch (error) {
      setStatus({
        type: "error",
        message: error instanceof Error ? error.message : "Could not register passkey.",
      })
    }
  }

  async function remove(id: string) {
    try {
      await authClient.passkey.deletePasskey({ id })
      await refresh()
    } catch {
      // best-effort
    }
  }

  return (
    <section id="security" className="mt-16 scroll-mt-24">
      <div className="mb-7">
        <h2 className="text-[27px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white">
          Security
        </h2>
        <p className="mt-2 max-w-[640px] text-[13px] leading-5 text-[#6A6E77] dark:text-[#A9ADB6]">
          Register a passkey for passwordless, phishing-resistant sign-in with Face ID,
          Touch ID, Windows Hello, or a hardware security key.
        </p>
      </div>

      <div className="mb-5 flex flex-col gap-3 sm:flex-row sm:items-center">
        <VelionInput
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="Passkey name (e.g. MacBook, iPhone)"
          aria-label="Passkey name"
          className="sm:max-w-[320px]"
        />
        <VelionButton
          onClick={register}
          disabled={status.type === "working"}
          className="shrink-0"
        >
          {status.type === "working" ? "Waiting for device…" : "Register a passkey"}
        </VelionButton>
      </div>

      {status.type === "error" ? (
        <p className="mb-4 text-[12px] text-[#C0362C]">{status.message}</p>
      ) : null}
      {status.type === "success" ? (
        <p className="mb-4 inline-flex items-center gap-1 text-[12px] text-[#1A7F4B]">
          <Check className="size-3.5" /> {status.message}
        </p>
      ) : null}

      <div className="divide-y divide-[#E8E8EA] overflow-hidden rounded-[18px] border border-[#E1E2E4] bg-white/42 dark:divide-white/10 dark:border-white/10 dark:bg-white/5">
        {passkeys === null ? (
          <p className="px-5 py-4 text-[12px] text-[#737780] dark:text-[#A9ADB6]">
            Loading passkeys…
          </p>
        ) : passkeys.length === 0 ? (
          <p className="px-5 py-4 text-[12px] text-[#737780] dark:text-[#A9ADB6]">
            No passkeys registered yet.
          </p>
        ) : (
          passkeys.map((passkey) => (
            <div
              key={passkey.id}
              className="flex items-center justify-between gap-4 px-5 py-4"
            >
              <div className="min-w-0">
                <p className="text-[13px] font-medium text-[#111111] dark:text-white">
                  {passkey.name || "Passkey"}
                </p>
                {formatDate(passkey.createdAt) ? (
                  <p className="mt-1 truncate text-[12px] text-[#737780] dark:text-[#A9ADB6]">
                    Added {formatDate(passkey.createdAt)}
                  </p>
                ) : null}
              </div>
              <VelionButton
                size="sm"
                radius="sm"
                onClick={() => remove(passkey.id)}
                className="shrink-0 px-3 text-[12px]"
              >
                Remove
              </VelionButton>
            </div>
          ))
        )}
      </div>
    </section>
  )
}
