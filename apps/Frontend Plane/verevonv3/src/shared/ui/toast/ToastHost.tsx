import './toast.css'
import { For, Show } from 'solid-js'
import { AlertCircle, AlertTriangle, CheckCircle2, Info, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { dismissToast, pauseToast, resumeToast, shellToasts, type ToastTone } from '@/shared/ui/toast/toast-store'

function toneIcon(tone: ToastTone) {
  switch (tone) {
    case 'success':
      return <CheckCircle2 size={16} aria-hidden="true" />
    case 'warning':
      return <AlertTriangle size={16} aria-hidden="true" />
    case 'error':
      return <AlertCircle size={16} aria-hidden="true" />
    default:
      return <Info size={16} aria-hidden="true" />
  }
}

/** Moves focus to a neighbouring toast before the focused one is removed, so focus does not drop to the page. */
function focusNeighbour(root: HTMLElement | undefined): void {
  const stack = root?.parentElement
  if (!root || !stack) return
  const toasts = Array.from(stack.querySelectorAll<HTMLElement>('[data-shell-toast]'))
  const index = toasts.indexOf(root)
  const neighbour = toasts[index + 1] ?? toasts[index - 1]
  neighbour?.focus()
}

/**
 * The shell's toast stack (F-t3-078). CoreShell mounts it once, behind `VITE_SHELL_TOASTS=true`. The stack is a
 * polite live region, so a new toast is announced without moving focus; an error toast is an alert.
 *
 * Each toast can be reached with Tab. Escape dismisses the focused one. Hovering or focusing a toast holds its
 * countdown until the pointer leaves and focus moves on.
 */
export function ToastHost() {
  const i18n = useI18n()
  const toneLabel = (tone: ToastTone): string => {
    switch (tone) {
      case 'success':
        return i18n.tr('Suksess', 'Success')
      case 'warning':
        return i18n.tr('Advarsel', 'Warning')
      case 'error':
        return i18n.tr('Feil', 'Error')
      default:
        return i18n.tr('Info', 'Info')
    }
  }

  return (
    <div class="shell-toasts" aria-live="polite" aria-relevant="additions text">
      <For each={shellToasts()} keyed={(toast) => toast.id}>
        {(toast) => {
          let root: HTMLDivElement | undefined
          const id = () => toast().id
          const dismiss = () => {
            focusNeighbour(root)
            dismissToast(id())
          }
          return (
            <div
              ref={(element) => {
                root = element
              }}
              class={`shell-toast shell-toast--${toast().tone}`}
              role={toast().tone === 'error' ? 'alert' : undefined}
              aria-live={toast().tone === 'error' ? 'assertive' : undefined}
              aria-atomic="true"
              tabindex="0"
              data-shell-toast={id()}
              onKeyDown={(event) => {
                if (event.key !== 'Escape') return
                event.stopPropagation()
                dismiss()
              }}
              onMouseEnter={() => pauseToast(id(), 'hover')}
              onMouseLeave={() => resumeToast(id(), 'hover')}
              onFocusIn={() => pauseToast(id(), 'focus')}
              onFocusOut={(event) => {
                if (event.relatedTarget instanceof Node && root?.contains(event.relatedTarget)) return
                resumeToast(id(), 'focus')
              }}
            >
              <span class="shell-toast__icon">{toneIcon(toast().tone)}</span>
              <div class="shell-toast__body">
                <span class="sr-only">{toneLabel(toast().tone)}: </span>
                {toast().message}
              </div>
              <Show when={toast().action}>
                {(action) => (
                  <button
                    type="button"
                    class="shell-toast__action"
                    onClick={() => {
                      const run = action().run
                      dismiss()
                      run()
                    }}
                  >
                    {action().label}
                  </button>
                )}
              </Show>
              <button
                type="button"
                class="shell-toast__close"
                aria-label={i18n.tr('Lukk varsel', 'Dismiss notification')}
                onClick={dismiss}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </div>
          )
        }}
      </For>
    </div>
  )
}
