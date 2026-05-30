'use client'

import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { brregService, formatBrregAddress, type BrregEnhet } from '@/lib/services/brreg-service'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useLanguageSwitch } from '@/components/auth/lib/i18n/hooks'

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

const COPY = {
  nb: {
    verify: 'Verifiser i Enhetsregisteret',
    optional: '(valgfritt)',
    skip: 'Hopp over verifisering',
    placeholder: 'Søk på organisasjonsnavn...',
    searching: 'Søker...',
    unreachable: 'Kunne ikke nå Enhetsregisteret',
    empty: 'Ingen organisasjoner funnet',
    orgNumber: 'Org.nr',
    employees: 'ansatte',
    bankrupt: 'Konkurs',
    closing: 'Under avvikling',
    verified: 'Verifisert',
    change: 'Bytt',
  },
  en: {
    verify: 'Verify in the Norwegian business registry',
    optional: '(optional)',
    skip: 'Skip verification',
    placeholder: 'Search by organization name...',
    searching: 'Searching...',
    unreachable: 'Could not reach the business registry',
    empty: 'No organizations found',
    orgNumber: 'Org no.',
    employees: 'employees',
    bankrupt: 'Bankrupt',
    closing: 'Being dissolved',
    verified: 'Verified',
    change: 'Change',
  },
} as const

export function BrregSearch({ initialQuery = '', onSelect, onSkip }: BrregSearchProps) {
  const { currentLocale } = useLanguageSwitch()
  const copy = COPY[currentLocale === 'en' ? 'en' : 'nb']
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
          {copy.verify}
          <span className="ml-1 text-muted-foreground font-normal">{copy.optional}</span>
        </span>
        <button
          type="button"
          data-testid="onboarding-brreg-skip"
          onClick={onSkip}
          className="text-xs text-muted-foreground hover:text-foreground underline underline-offset-2"
        >
          {copy.skip}
        </button>
      </div>

      <div className="relative">
        <Input
          value={query}
          onChange={(e) => {
            setSelected(null)
            setQuery(e.target.value)
          }}
          placeholder={copy.placeholder}
          autoComplete="off"
        />

        {isFetching && (
          <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground text-xs">
            {copy.searching}
          </span>
        )}

        {showDropdown && (
          <ul
            ref={listRef}
            className="absolute z-50 mt-1 w-full rounded-md border bg-popover shadow-md max-h-72 overflow-y-auto text-sm"
          >
            {isError && (
              <li className="px-3 py-2 text-destructive">{copy.unreachable}</li>
            )}
            {!isFetching && !isError && results?.length === 0 && (
              <li className="px-3 py-2 text-muted-foreground">{copy.empty}</li>
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
                        {copy.orgNumber} {enhet.organisasjonsnummer}
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
                        {enhet.antallAnsatte} {copy.employees}
                      </span>
                    )}
                  </div>
                  {(enhet.konkurs || enhet.underAvvikling) && (
                    <p className="text-xs text-destructive mt-0.5">
                      {enhet.konkurs ? copy.bankrupt : copy.closing}
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
              {copy.orgNumber} {selected.organisasjonsnummer} · {copy.verified}
            </p>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={handleClear}
            className="shrink-0 h-6 px-2 text-xs text-muted-foreground"
          >
            {copy.change}
          </Button>
        </div>
      )}
    </div>
  )
}
