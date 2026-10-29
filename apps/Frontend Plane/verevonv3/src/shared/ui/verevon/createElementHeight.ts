import { createSignal, onCleanup } from 'solid-js'

export function createElementHeight<TElement extends HTMLElement>() {
  const [height, setHeight] = createSignal<number>()
  let observer: ResizeObserver | undefined

  const setElement = (element?: TElement) => {
    observer?.disconnect()
    observer = undefined

    if (!element || typeof window === 'undefined') return

    const update = () => setHeight(element.scrollHeight)

    if (typeof ResizeObserver === 'undefined') {
      window.requestAnimationFrame(update)
      return
    }

    observer = new ResizeObserver(update)
    observer.observe(element)
    window.requestAnimationFrame(update)
  }

  onCleanup(() => observer?.disconnect())

  return {
    height,
    setElement,
  }
}
