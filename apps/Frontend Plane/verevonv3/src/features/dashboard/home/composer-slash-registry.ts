import { createSignal, type Accessor, type Component } from 'solid-js'
import type { LucideProps } from '@/shared/icons'

/**
 * The composer's slash-command registry (Wave 3 C-B10): one per composer, created by the composer and
 * handed to the pieces that bring their own `/` command, so a feature registers its command where it
 * lives instead of growing the composer's built-in list. EP-3 S5's `/kontekst` is the first entry;
 * slash templates and `/learn` (EP-11 S6/S7) register here too. `@` is not a slash command and stays
 * reserved for Space agents (DEC-26).
 */
export type RegisteredSlashCommand = {
  /** Unique across the registry; also the menu item's id. */
  id: string
  /** Canonical token after "/", lowercase and space-free. */
  name: string
  aliases: string[]
  label: string
  description: { no: string; en: string }
  icon: Component<LucideProps>
  /** What picking the command does. The `/name` the person typed is cleared first. */
  run: () => void
}

export type SlashCommandRegistry = {
  /** The commands registered right now, in registration order. */
  commands: Accessor<readonly RegisteredSlashCommand[]>
  /** Adds a command (replacing one with the same id) and returns the function that removes it. */
  register: (command: RegisteredSlashCommand) => () => void
}

export function createSlashCommandRegistry(): SlashCommandRegistry {
  const [commands, setCommands] = createSignal<readonly RegisteredSlashCommand[]>([])
  return {
    commands,
    register(command) {
      setCommands((current) => [...current.filter((item) => item.id !== command.id), command])
      return () => setCommands((current) => current.filter((item) => item !== command))
    },
  }
}
