/** Implements automatic-activation keyboard navigation for a local ARIA tablist. */
export function handleTabKeyDown(event: KeyboardEvent & { currentTarget: HTMLElement }): void {
  const tablist = event.currentTarget.closest<HTMLElement>('[role="tablist"]')
  if (!tablist) return
  const tabs = [...tablist.querySelectorAll<HTMLElement>('[role="tab"]:not([disabled])')]
  const currentIndex = tabs.indexOf(event.currentTarget)
  if (currentIndex < 0 || tabs.length < 2) return

  let targetIndex: number | null = null
  if (event.key === 'ArrowRight' || event.key === 'ArrowDown') targetIndex = (currentIndex + 1) % tabs.length
  if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') targetIndex = (currentIndex - 1 + tabs.length) % tabs.length
  if (event.key === 'Home') targetIndex = 0
  if (event.key === 'End') targetIndex = tabs.length - 1
  if (targetIndex === null) return

  event.preventDefault()
  const target = tabs[targetIndex]
  target?.focus()
  target?.click()
}
