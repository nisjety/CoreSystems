import { createSignal, onCleanup } from 'solid-js'
import { streamCrawlPreview, type SseHandlers } from '@/features/onboarding/lib/api'

type CrawlPreviewInput = {
  brief?: string
  maxPages?: number
  orgId?: string
  url: string
}

export function createCrawlPreviewStream() {
  const [previewing, setPreviewing] = createSignal(false)
  let controller: AbortController | undefined

  const abort = () => {
    controller?.abort()
    controller = undefined
    setPreviewing(false)
  }

  const start = async (input: CrawlPreviewInput, handlers: SseHandlers) => {
    abort()
    const currentController = new AbortController()
    controller = currentController
    setPreviewing(true)

    try {
      await streamCrawlPreview(input, handlers, currentController.signal)
    } catch (reason) {
      if (currentController.signal.aborted) return
      throw reason
    } finally {
      if (controller === currentController) {
        controller = undefined
        setPreviewing(false)
      }
    }
  }

  onCleanup(abort)

  return {
    abort,
    previewing,
    start,
  }
}
