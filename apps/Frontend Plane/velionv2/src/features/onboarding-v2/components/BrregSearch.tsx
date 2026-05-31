"use client";

import { useEffect, useRef, useState } from "react";
import { brregService, formatBrregAddress, type BrregEnhet } from "@/lib/services/brreg-service";

interface BrregSearchProps {
  /** Pre-filled search term (usually the org name already typed). */
  initialQuery?: string;
  /** Called when the user selects a match from the registry. */
  onSelect: (enhet: BrregEnhet) => void;
  /** Called when the user wants to enter the name manually without verifying. */
  onManualEntry?: () => void;
}

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState<T>(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return debounced;
}

const COPY = {
  label: "Verifiser i Enhetsregisteret",
  optional: "(valgfritt)",
  skip: "Hopp over, jeg skriver inn manuelt",
  placeholder: "Søk på organisasjonsnavn…",
  searching: "Søker…",
  unreachable: "Kunne ikke nå Enhetsregisteret",
  empty: "Ingen organisasjoner funnet",
  orgNumber: "Org.nr",
  employees: "ansatte",
  bankrupt: "Konkurs",
  closing: "Under avvikling",
  verified: "Verifisert",
  change: "Bytt",
} as const;

export function BrregSearch({ initialQuery = "", onSelect, onManualEntry }: BrregSearchProps) {
  const [query, setQuery] = useState(initialQuery);
  const [selected, setSelected] = useState<BrregEnhet | null>(null);
  const [results, setResults] = useState<BrregEnhet[]>([]);
  const [isFetching, setIsFetching] = useState(false);
  const [isError, setIsError] = useState(false);
  const debouncedQuery = useDebounce(query, 300);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    const trimmed = debouncedQuery.trim();

    if (trimmed.length < 2 || selected) {
      const t = setTimeout(() => {
        setResults([]);
        setIsError(false);
        setIsFetching(false);
      }, 0);
      return () => clearTimeout(t);
    }

    const controller = new AbortController();

    const loadingTimer = setTimeout(() => {
      setIsFetching(true);
      setIsError(false);
    }, 0);

    brregService
      .searchByName(trimmed, 10, controller.signal)
      .then((data) => {
        setResults(data);
        setIsFetching(false);
      })
      .catch((err: unknown) => {
        if (
          controller.signal.aborted ||
          (err instanceof Error && err.name === "AbortError")
        ) {
          return; // request was cancelled — do not update state
        }
        setIsError(true);
        setIsFetching(false);
      });

    return () => {
      clearTimeout(loadingTimer);
      controller.abort();
    };
  }, [debouncedQuery, selected]);

  const showDropdown = debouncedQuery.trim().length >= 2 && !selected;

  const handleSelect = (enhet: BrregEnhet) => {
    setSelected(enhet);
    setQuery(enhet.navn);
    setResults([]);
    onSelect(enhet);
  };

  const handleClear = () => {
    setSelected(null);
    setQuery("");
    setResults([]);
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="relative">
        <span className="block text-[11px] uppercase tracking-[0.16em] text-[#6B6660]">
          {COPY.label}{" "}
          <span className="normal-case tracking-normal text-[#A09890]">{COPY.optional}</span>
        </span>
        <input
          type="search"
          autoComplete="off"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            if (selected) setSelected(null);
          }}
          placeholder={COPY.placeholder}
          className="mt-2 w-full rounded-md border border-[#D6D2CB] bg-white px-3 py-2.5 text-[14px] text-[#1F1B17] placeholder:text-[#A09890] focus:border-[#1F1B17] focus:outline-none"
        />
        {isFetching && (
          <span className="absolute right-3 top-[calc(50%+6px)] -translate-y-1/2">
            <span className="block size-4 animate-spin rounded-full border-2 border-[#D6D2CB] border-t-[#1F1B17]" />
          </span>
        )}

        {showDropdown && (
          <ul
            ref={listRef}
            className="absolute z-50 mt-1 max-h-72 w-full overflow-y-auto rounded-md border border-[#D6D2CB] bg-white shadow-[0_8px_18px_rgba(31,27,23,0.10)] text-sm"
          >
            {isError && (
              <li className="px-3 py-2 text-[13px] text-[#9A3412]">{COPY.unreachable}</li>
            )}
            {!isFetching && !isError && results.length === 0 && (
              <li className="px-3 py-2 text-[13px] text-[#A09890]">{COPY.empty}</li>
            )}
            {results.map((enhet) => (
              <li key={enhet.organisasjonsnummer}>
                <button
                  type="button"
                  onClick={() => handleSelect(enhet)}
                  className="w-full text-left px-3 py-2.5 text-[13px] hover:bg-[#F7F4ED] transition-colors"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <p className="font-medium text-[#1F1B17] truncate">{enhet.navn}</p>
                      <p className="text-[11px] text-[#6B6660]">
                        {COPY.orgNumber} {enhet.organisasjonsnummer}
                        {enhet.organisasjonsform?.beskrivelse
                          ? ` · ${enhet.organisasjonsform.beskrivelse}`
                          : ""}
                      </p>
                      {enhet.forretningsadresse && (
                        <p className="text-[11px] text-[#A09890] truncate">
                          {formatBrregAddress(enhet.forretningsadresse)}
                        </p>
                      )}
                    </div>
                    {enhet.antallAnsatte != null && (
                      <span className="shrink-0 text-[11px] text-[#A09890]">
                        {enhet.antallAnsatte} {COPY.employees}
                      </span>
                    )}
                  </div>
                  {(enhet.konkurs ?? enhet.underAvvikling) && (
                    <p className="mt-0.5 text-[11px] text-[#9A3412]">
                      {enhet.konkurs ? COPY.bankrupt : COPY.closing}
                    </p>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {selected && (
        <div className="flex items-center justify-between rounded-md border border-[#D6D2CB] bg-[#F7F4ED] px-3 py-2">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-[#1F1B17] truncate">{selected.navn}</p>
            <p className="text-[11px] text-[#6B6660]">
              {COPY.orgNumber} {selected.organisasjonsnummer} · {COPY.verified}
            </p>
          </div>
          <button
            type="button"
            onClick={handleClear}
            className="ml-3 shrink-0 text-[11px] uppercase tracking-[0.16em] text-[#A09890] transition-colors hover:text-[#1F1B17]"
          >
            {COPY.change}
          </button>
        </div>
      )}

      {onManualEntry && !selected && (
        <button
          type="button"
          onClick={onManualEntry}
          className="self-start text-[11px] uppercase tracking-[0.18em] text-[#A09890] transition-colors hover:text-[#111111]"
        >
          {COPY.skip}
        </button>
      )}
    </div>
  );
}
