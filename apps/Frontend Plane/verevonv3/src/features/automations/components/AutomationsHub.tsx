import { AutomationsList } from './AutomationsList'

/**
 * "Automatiseringer" (EP-6-S1): the signed-in person's own schedules, filtered Active, Paused
 * and Expired. The full mount of the shared list (EP-6-S4): the rows, filters and WS20 guards
 * live in AutomationsList, which the chat panel and Settings mount too. The hub adds no server
 * behaviour of its own.
 */
export function AutomationsHub() {
  return <AutomationsList scope="mine" />
}
