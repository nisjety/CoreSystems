'use client'

import { useEffect, useState } from 'react'
import { ZammadClient, ZammadTicket, createZammadClientFromEnv } from '@/lib/clients/zammad-client'

export function useZammadClient() {
  const [client, setClient] = useState<ZammadClient | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    try {
      setClient(createZammadClientFromEnv())
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to initialize Zammad client')
    }
  }, [])

  return { client, error }
}

export function useZammadTickets() {
  const { client } = useZammadClient()
  const [tickets, setTickets] = useState<ZammadTicket[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = async () => {
    if (!client) return
    try {
      setLoading(true)
      const data = await client.getTickets()
      setTickets(data)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch tickets')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    refresh()
  }, [client])

  return { tickets, loading, error, refresh }
}

export function useCreateZammadTicket() {
  const { client } = useZammadClient()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const create = async (data: any) => {
    if (!client) throw new Error('Zammad client not initialized')
    try {
      setLoading(true)
      const ticket = await client.createTicket(data)
      setError(null)
      return ticket
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to create ticket'
      setError(message)
      throw err
    } finally {
      setLoading(false)
    }
  }

  return { create, loading, error }
}

export function useZammadHealth() {
  const { client } = useZammadClient()
  const [healthy, setHealthy] = useState(false)
  const [checking, setChecking] = useState(true)

  useEffect(() => {
    if (!client) return

    const check = async () => {
      try {
        const result = await client.health()
        setHealthy(result.ok)
      } catch {
        setHealthy(false)
      } finally {
        setChecking(false)
      }
    }

    check()
  }, [client])

  return { healthy, checking }
}
