import { fireEvent as dispatch } from '@solidjs/testing-library'
import { flush } from 'solid-js'

// Solid 2 commits event-driven updates on its scheduler. Tests that inspect the
// DOM immediately after an event must flush that scheduler before asserting.
export const fireEvent: typeof dispatch = new Proxy(dispatch, {
  apply(target, receiver, args) {
    flush()
    return flush(() => Reflect.apply(target, receiver, args))
  },
  get(target, key, receiver) {
    const value: unknown = Reflect.get(target, key, receiver)
    return typeof value === 'function'
      ? (...args: unknown[]) => { flush(); return flush(() => Reflect.apply(value, target, args)) }
      : value
  },
})
