import { render as mount } from '@solidjs/testing-library'
import { flush } from 'solid-js'

// Flush initial Solid 2 effects before a recovered synchronous UI test starts
// dispatching events. Production receives this flush before the next task.
export const render: typeof mount = (...args) => {
  const result = mount(...args)
  flush()
  return result
}
