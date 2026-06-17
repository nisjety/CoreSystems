'use client'

import { useEffect, useState } from 'react'
import { NohuClient, NohuWorkflow, NohuExecution, createNohuClientFromEnv } from '@/lib/clients/nohu-client'

export function useNohuClient() {
  const [client, setClient] = useState<NohuClient | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    try {
      setClient(createNohuClientFromEnv())
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to initialize Nohu client')
    }
  }, [])

  return { client, error }
}

export function useNohuWorkflows() {
  const { client } = useNohuClient()
  const [workflows, setWorkflows] = useState<NohuWorkflow[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = async () => {
    if (!client) return
    try {
      setLoading(true)
      const data = await client.listWorkflows()
      setWorkflows(Array.isArray(data) ? data : [])
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch workflows')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    refresh()
  }, [client])

  return { workflows, loading, error, refresh }
}

export function useExecuteWorkflow() {
  const { client } = useNohuClient()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const execute = async (workflowId: string, input: Record<string, any>) => {
    if (!client) throw new Error('Nohu client not initialized')
    try {
      setLoading(true)
      const execution = await client.executeWorkflow(workflowId, input)
      setError(null)
      return execution
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to execute workflow'
      setError(message)
      throw err
    } finally {
      setLoading(false)
    }
  }

  return { execute, loading, error }
}

export function useNohuExecution(executionId: string | null) {
  const { client } = useNohuClient()
  const [execution, setExecution] = useState<NohuExecution | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = async () => {
    if (!client || !executionId) return
    try {
      setLoading(true)
      const data = await client.getExecution(executionId)
      setExecution(data)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch execution')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (!executionId) return

    // Fetch immediately
    refresh()

    // Auto-poll every 2 seconds while running
    const interval = setInterval(() => {
      if (execution?.status === 'running' || execution?.status === 'pending') {
        refresh()
      } else if (interval) {
        clearInterval(interval)
      }
    }, 2000)

    return () => clearInterval(interval)
  }, [executionId, client])

  return { execution, loading, error, refresh }
}

export function useNohuHealth() {
  const { client } = useNohuClient()
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
