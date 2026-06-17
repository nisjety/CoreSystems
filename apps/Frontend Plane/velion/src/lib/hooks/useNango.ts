'use client'

import { useEffect, useState } from 'react'
import { NangoClient, NangoIntegration, NangoConnection, createNangoClientFromEnv } from '@/lib/clients/nango-client'

export function useNangoClient() {
  const [client, setClient] = useState<NangoClient | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    try {
      setClient(createNangoClientFromEnv())
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to initialize Nango client')
    }
  }, [])

  return { client, error }
}

export function useNangoIntegrations() {
  const { client } = useNangoClient()
  const [integrations, setIntegrations] = useState<NangoIntegration[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = async () => {
    if (!client) return
    try {
      setLoading(true)
      const data = await client.listIntegrations()
      setIntegrations(Array.isArray(data) ? data : [])
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch integrations')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    refresh()
  }, [client])

  return { integrations, loading, error, refresh }
}

export function useNangoConnections() {
  const { client } = useNangoClient()
  const [connections, setConnections] = useState<NangoConnection[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = async () => {
    if (!client) return
    try {
      setLoading(true)
      const data = await client.listConnections()
      setConnections(Array.isArray(data) ? data : [])
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch connections')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    refresh()
  }, [client])

  return { connections, loading, error, refresh }
}

export function useNangoOAuth() {
  const { client } = useNangoClient()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const initiateOAuth = async (provider: string, redirectTo: string, scopes?: string[]) => {
    if (!client) throw new Error('Nango client not initialized')
    try {
      setLoading(true)
      const url = await client.initiateOAuth(provider, redirectTo, scopes)
      setError(null)
      return url
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to initiate OAuth'
      setError(message)
      throw err
    } finally {
      setLoading(false)
    }
  }

  return { initiateOAuth, loading, error }
}

export function useNangoHealth() {
  const { client } = useNangoClient()
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
