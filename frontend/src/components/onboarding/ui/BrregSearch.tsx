'use client'

import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { brregService, formatBrregAddress, type BrregEnhet } from '@/lib/services/brreg-service'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface BrregSearchProps {
  /** Pre-filled search term (usually the org name already typed). */
  initialQuery?: string
  /** Called when the user selects a Brreg match. */
  onSelect: (enhet: BrregEnhet) => void
  /** Called when the user explicitly skips verification. */
  onSkip: () => void
}

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), delay)
    return () => clearTimeout(t)
  }, [value, delay])
  return debounced
}

export function BrregSearch({ initialQuery = '', onSelect, onSkip }: BrregSearchProps) {
  const [query, setQuery] = useState(initialQuery)
  const [selected, setSelected] = useState<BrregEnhet | null>(null)
  const debouncedQuery = useDebounce(query, 300)
  const listRef = useRef<HTMLUListElement>(null)

  const { data: results, isFetching, isError } = useQuery({
    queryKey: ['brreg-search', debouncedQuery],
    queryFn: () => brregService.searchByName(debouncedQuery, 10),
    enabled: debouncedQuery.trim().length > 1,
    staleTime: 60_000,
  })

  const showDropdown = debouncedQuery.trim().length > 1 && !selected

  const handleSelect = (enhet: BrregEnhet) => {
    setSelected(enhet)
    setQuery(enhet.navn)
    onSelect(enhet)
  }

  const handleClear = () => {
    setSelected(null)
    setQuery('')
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium leading-none">
          Verifiser i Enhetsregisteret
          <span className="ml-1 text-muted-foreground font-normal">(valgfritt)</span>
        </span>
        <button
          type="button"
          onClick={onSkip}
          className="text-xs text-muted-foreground hover:text-foreground underline underline-offset-2"
        >
          Hopp over verifisering
        </button>
      </div>

      <div className="relative">
        <Input
          value={query}
          onChange={(e) => {
            setSelected(null)
            setQuery(e.target.value)
          }}
          placeholder="Søk på organisasjonsnavn…"
          autoComplete="off"
        />

        {isFetching && (
          <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground text-xs">
            Søker…
          </span>
        )}

        {showDropdown && (
          <ul
            ref={listRef}
            className="absolute z-50 mt-1 w-full rounded-md border bg-popover shadow-md max-h-72 overflow-y-auto text-sm"
          >
            {isError && (
              <li className="px-3 py-2 text-destructive">Kunne ikke nå Enhetsregisteret</li>
            )}
            {!isFetching && !isError && results?.length === 0 && (
              <li className="px-3 py-2 text-muted-foreground">Ingen organisasjoner funnet</li>
            )}
            {results?.map((enhet) => (
              <li key={enhet.organisasjonsnummer}>
                <button
                  type="button"
                  onClick={() => handleSelect(enhet)}
                  className="w-full text-left px-3 py-2 hover:bg-accent hover:text-accent-foreground transition-colors"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <p className="font-medium truncate">{enhet.navn}</p>
                      <p className="text-xs text-muted-foreground">
                        Org.nr {enhet.organisasjonsnummer}
                        {enhet.organisasjonsform?.beskrivelse
                          ? ` · ${enhet.organisasjonsform.beskrivelse}`
                          : ''}
                      </p>
                      {enhet.forretningsadresse && (
                        <p className="text-xs text-muted-foreground truncate">
                          {formatBrregAddress(enhet.forretningsadresse)}
                        </p>
                      )}
                    </div>
                    {enhet.antallAnsatte != null && (
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {enhet.antallAnsatte} ansatte
                      </span>
                    )}
                  </div>
                  {(enhet.konkurs || enhet.underAvvikling) && (
                    <p className="text-xs text-destructive mt-0.5">
                      {enhet.konkurs ? 'Konkurs' : 'Under avvikling'}
                    </p>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {selected && (
        <div className="flex items-center justify-between rounded-md border border-green-200 bg-green-50 dark:border-green-800 dark:bg-green-950/30 px-3 py-2">
          <div className="min-w-0">
            <p className="text-sm font-medium text-green-800 dark:text-green-300 truncate">
              {selected.navn}
            </p>
            <p className="text-xs text-green-700 dark:text-green-400">
              Org.nr {selected.organisasjonsnummer} · Verifisert
            </p>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={handleClear}
            className="shrink-0 h-6 px-2 text-xs text-muted-foreground"
          >
            Bytt
          </Button>
        </div>
      )}
    </div>
  )
}
