"use client";

import {
  useCallback,
  useEffect,
  useReducer,
  useState,
  type PropsWithChildren,
  type ReactNode,
} from "react";
import { Activity, ArrowUpRight, CloudSun, Newspaper, RefreshCw, Wind } from "lucide-react";
import { VelionSelect } from "@/components/ui/velion-ui";
import type { DashboardCard } from "@/features/dashboard-v2/lib/dashboard-surface";
import {
  loadDashboardInformationSnapshot,
} from "@/features/dashboard-v2/lib/dashboard-information-client";
import {
  type BrowserLocationCoordinates,
  formatRelativeNorwegianTime,
  type InformationNewsPayload,
  type InformationTrafficPayload,
  type InformationWeatherPayload,
  newsCategoryOptions,
  requestBrowserLocation,
  weatherGlyph,
} from "@/features/dashboard-v2/lib/information-core";
import { apiGet } from "@/lib/api/client-envelope";
import { cn } from "@/lib/utils";

type InformationCardProps = {
  card: DashboardCard;
  onPrompt: (card: DashboardCard) => void;
  action: ReactNode;
};

type NewsCardState = {
  data: InformationNewsPayload | null;
  error: string | null;
  loading: boolean;
};

type NewsCardAction =
  | { type: "loading" }
  | { type: "loaded"; data: InformationNewsPayload | null; error: string | null }
  | { type: "error"; message: string };

function newsCardReducer(
  state: NewsCardState,
  action: NewsCardAction,
): NewsCardState {
  switch (action.type) {
    case "loading":
      return { ...state, error: null, loading: true };
    case "loaded":
      return {
        data: action.data,
        error: action.error,
        loading: false,
      };
    case "error":
      return { ...state, error: action.message, loading: false };
    default:
      return state;
  }
}

export function WeatherDashboardCard({
  card,
  onPrompt,
}: {
  card: DashboardCard;
  onPrompt: (card: DashboardCard) => void;
}) {
  const [data, setData] = useState<InformationWeatherPayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [usingLiveLocation, setUsingLiveLocation] = useState(false);

  const load = async (coordinates?: BrowserLocationCoordinates | null, forceRefreshLocation = false) => {
    setLoading(true);
    setError(null);
    try {
      const resolvedCoordinates =
        coordinates === undefined ? await requestBrowserLocation(forceRefreshLocation) : coordinates;

      const params = new URLSearchParams();
      if (resolvedCoordinates) {
        params.set("lat", resolvedCoordinates.latitude.toString());
        params.set("lon", resolvedCoordinates.longitude.toString());
        if (typeof resolvedCoordinates.altitude === "number") {
          params.set("altitude", Math.round(resolvedCoordinates.altitude).toString());
        }
      }

      const payload = await apiGet<InformationWeatherPayload>(
        `/api/v1/information/weather${params.size ? `?${params}` : ""}`,
        {
        credentials: "include",
        },
      );
      setUsingLiveLocation(Boolean(resolvedCoordinates));
      setData(payload);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Kunne ikke hente værdata.");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let cancelled = false;

    void loadDashboardInformationSnapshot()
      .then((snapshot) => {
        if (cancelled) return;
        setUsingLiveLocation(snapshot.usingLiveLocation);
        setData(snapshot.weather);
        setError(snapshot.weatherError);
        setLoading(false);
      })
      .catch(() => {
        if (!cancelled) {
          void load();
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <InformationCardShell
      card={card}
      onPrompt={onPrompt}
      action={
        <button
          type="button"
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            void load(undefined, true);
          }}
          className="grid size-8 place-items-center rounded-full bg-white/70 text-[#1A1A1A] transition hover:bg-white"
          aria-label="Oppdater vær"
          title="Oppdater vær"
        >
          <RefreshCw className={cn("size-4", loading ? "animate-spin" : "")} />
        </button>
      }
    >
      {loading && !data ? <InformationSkeleton lines={3} /> : null}
      {!loading && error ? <InformationError text={error} /> : null}
      {data ? (
        <div className="space-y-2.5">
          <div className="flex items-end justify-between gap-2.5">
            <div>
              <p className="text-[11px] font-medium uppercase tracking-[0.16em] text-[#6F8EA1]">
                {data.current.location}
              </p>
              <p className="mt-0.5 text-[10px] text-[#8AA0AF]">
                {usingLiveLocation ? "Din posisjon" : "Standard plassering"}
              </p>
              <div className="mt-1 flex items-end gap-2.5">
                <span className="text-[28px] leading-none text-[#194C67]">{weatherGlyph(data.current.condition)}</span>
                <div>
                  <p className="text-[24px] font-semibold leading-none text-[#163649]">
                    {data.current.temperature}°
                  </p>
                  <p className="mt-0.5 text-[11px] text-[#587487]">{data.current.condition}</p>
                </div>
              </div>
            </div>
            <div className="rounded-[14px] bg-white/70 px-2.5 py-1.5 text-right shadow-sm">
              <p className="text-[10px] uppercase tracking-[0.14em] text-[#7F97A7]">Oppdatert</p>
              <p className="mt-0.5 text-[11px] font-medium text-[#27475A]">
                {formatRelativeNorwegianTime(data.current.lastUpdated)}
              </p>
            </div>
          </div>

          <div className="grid grid-cols-3 gap-1.5">
            <MetricPill icon={<Wind className="size-3.5" />} label="Vind" value={`${data.current.windSpeed} m/s`} />
            <MetricPill icon={<CloudSun className="size-3.5" />} label="Fukt" value={`${data.current.humidity}%`} />
            <MetricPill icon={<Activity className="size-3.5" />} label="Trykk" value={`${data.current.pressure} hPa`} />
          </div>
        </div>
      ) : null}
    </InformationCardShell>
  );
}

export function TrafficDashboardCard({
  card,
  onPrompt,
}: {
  card: DashboardCard;
  onPrompt: (card: DashboardCard) => void;
}) {
  const [data, setData] = useState<InformationTrafficPayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [usingLiveLocation, setUsingLiveLocation] = useState(false);

  const load = async (coordinates?: BrowserLocationCoordinates | null, forceRefreshLocation = false) => {
    setLoading(true);
    setError(null);
    try {
      const resolvedCoordinates =
        coordinates === undefined ? await requestBrowserLocation(forceRefreshLocation) : coordinates;

      const params = new URLSearchParams({ radius: "40" });
      if (resolvedCoordinates) {
        params.set("lat", resolvedCoordinates.latitude.toString());
        params.set("lon", resolvedCoordinates.longitude.toString());
      }

      const payload = await apiGet<InformationTrafficPayload>(`/api/v1/information/traffic?${params}`, {
        credentials: "include",
      });
      setUsingLiveLocation(Boolean(resolvedCoordinates));
      setData(payload);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Kunne ikke hente trafikkdata.");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let cancelled = false;

    void loadDashboardInformationSnapshot()
      .then((snapshot) => {
        if (cancelled) return;
        setUsingLiveLocation(snapshot.usingLiveLocation);
        setData(snapshot.traffic);
        setError(snapshot.trafficError);
        setLoading(false);
      })
      .catch(() => {
        if (!cancelled) {
          void load();
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <InformationCardShell
      card={card}
      onPrompt={onPrompt}
      action={
        <button
          type="button"
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            void load(undefined, true);
          }}
          className="grid size-8 place-items-center rounded-full bg-white/70 text-[#1A1A1A] transition hover:bg-white"
          aria-label="Oppdater trafikk"
          title="Oppdater trafikk"
        >
          <RefreshCw className={cn("size-4", loading ? "animate-spin" : "")} />
        </button>
      }
    >
      {loading && !data ? <InformationSkeleton lines={3} /> : null}
      {!loading && error ? <InformationError text={error} /> : null}
      {data ? (
        <div className="space-y-2.5">
          <div className="flex items-center justify-between rounded-[14px] bg-white/70 px-2.5 py-2 shadow-sm">
            <div>
              <p className="text-[10px] uppercase tracking-[0.16em] text-[#8C5A46]">Statens vegvesen</p>
              <p className="mt-0.5 text-[13px] font-semibold text-[#3B231A]">{data.data.length} aktive målepunkter</p>
              <p className="mt-0.5 text-[10px] text-[#9B7A6C]">{usingLiveLocation ? "Nær deg" : "Oslo-område"}</p>
            </div>
            <p className="text-[11px] text-[#7D5E52]">{formatRelativeNorwegianTime(data.timestamp)}</p>
          </div>

          <div className="space-y-1.5">
            {data.data.slice(0, 2).map((station) => (
              <div key={station.id} className="rounded-[14px] bg-white/62 px-2.5 py-2 shadow-sm">
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-[12px] font-semibold text-[#2B211F]">{station.name}</p>
                    <p className="mt-0.5 text-[10px] text-[#8D7064]">
                      {station.roadReference} · {station.county}
                    </p>
                  </div>
                  <div className="text-right">
                    <p className="text-[11px] font-medium text-[#3B231A]">{station.averageSpeed} km/t</p>
                    <p className="text-[10px] text-[#8D7064]">{station.trafficVolume.toLocaleString("no-NO")} biler</p>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </InformationCardShell>
  );
}

export function NewsDashboardCard({
  card,
  onPrompt,
}: {
  card: DashboardCard;
  onPrompt: (card: DashboardCard) => void;
}) {
  const [{ data, error, loading }, dispatchNews] = useReducer(
    newsCardReducer,
    {
      data: null,
      error: null,
      loading: true,
    },
  );
  const [category, setCategory] = useState<(typeof newsCategoryOptions)[number]["value"]>("all");

  const load = useCallback(async (nextCategory = category) => {
    dispatchNews({ type: "loading" });
    try {
      const params = new URLSearchParams({
        limit: "8",
        maxAge: "24",
      });
      if (nextCategory !== "all") {
        params.set("category", nextCategory);
      }

      const payload = await apiGet<InformationNewsPayload>(`/api/v1/information/news?${params}`, {
        credentials: "include",
      });
      dispatchNews({ type: "loaded", data: payload, error: null });
    } catch (nextError) {
      dispatchNews({
        type: "error",
        message:
          nextError instanceof Error
            ? nextError.message
            : "Kunne ikke hente nyheter.",
      });
    }
  }, [category]);

  const loadSharedNews = useCallback(async () => {
    dispatchNews({ type: "loading" });
    try {
      const snapshot = await loadDashboardInformationSnapshot();
      dispatchNews({
        type: "loaded",
        data: snapshot.news,
        error: snapshot.newsError,
      });
    } catch (nextError) {
      dispatchNews({
        type: "error",
        message:
          nextError instanceof Error
            ? nextError.message
            : "Kunne ikke hente nyheter.",
      });
    }
  }, []);

  useEffect(() => {
    if (category === "all") {
      void loadSharedNews();
      return;
    }

    void load(category);
  }, [category, load, loadSharedNews]);

  return (
    <InformationCardShell
      card={card}
      onPrompt={onPrompt}
      action={
        <button
          type="button"
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            void load(category);
          }}
          className="grid size-8 place-items-center rounded-full bg-white/70 text-[#1A1A1A] transition hover:bg-white"
          aria-label="Oppdater nyheter"
          title="Oppdater nyheter"
        >
          <RefreshCw className={cn("size-4", loading ? "animate-spin" : "")} />
        </button>
      }
    >
      {loading && !data ? <InformationSkeleton lines={4} /> : null}
      {!loading && error ? <InformationError text={error} /> : null}
      <div className="space-y-1.5">
        <VelionSelect
          aria-label="Filtrer nyheter"
          value={category}
          variant="compact"
          className="h-9 rounded-[12px] border-none bg-white/70 px-3 text-[11px] font-medium text-[#50545C] shadow-sm"
          onChange={(event) => setCategory(event.target.value as (typeof newsCategoryOptions)[number]["value"])}
        >
          {newsCategoryOptions.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </VelionSelect>

        {data ? (
          <div className="max-h-[122px] space-y-1.5 overflow-y-auto pr-1">
            {data.articles.map((article) => (
              <a
                key={article.id}
                href={article.link}
                target="_blank"
                rel="noopener noreferrer"
                className="block rounded-[14px] bg-white/62 px-2.5 py-2 shadow-sm transition hover:bg-white"
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="min-w-0">
                    <p className="line-clamp-2 text-[11px] font-semibold leading-snug text-[#1E2228]">
                      {article.title}
                    </p>
                  </div>
                  <ArrowUpRight className="mt-0.5 size-3.5 shrink-0 text-[#8A8E96]" />
                </div>
                <div className="mt-1 flex items-center gap-2 text-[10px] text-[#8A8E96]">
                  <span>{article.source}</span>
                  <span>•</span>
                  <span>{formatRelativeNorwegianTime(article.publishDate)}</span>
                </div>
              </a>
            ))}
          </div>
        ) : null}
      </div>
    </InformationCardShell>
  );
}

function InformationCardShell({ action, card, children, onPrompt }: PropsWithChildren<InformationCardProps>) {
  return (
    <div className="velion-dashboard-card group relative h-full overflow-hidden rounded-[18px] bg-white p-2.5 shadow-[0_2px_10px_rgba(0,0,0,0.05)] transition-transform duration-300 hover:-translate-y-0.5 dark:bg-[#141516]">
      <div className="velion-dashboard-card-label pointer-events-none absolute left-0 top-0 z-30 bg-white px-5 pb-4 pt-5 text-[11px] font-semibold tracking-wide text-[#1A1A1A] dark:bg-[#141516] dark:text-white">
        {card.category}
      </div>

      <div className="relative h-full min-h-[228px] overflow-hidden rounded-[15px] bg-[radial-gradient(circle_at_top_left,rgba(255,255,255,0.96),rgba(246,241,233,0.9)_38%,rgba(231,224,214,0.88))] p-3 pt-14 dark:bg-[radial-gradient(circle_at_top_left,rgba(44,46,51,0.94),rgba(24,25,29,0.96)_55%,rgba(19,20,22,1))]">
        <div className="mb-3 flex items-start justify-between gap-3">
          <div className="min-w-0">
            <h3 className="text-[14px] font-semibold leading-snug text-[#1A1A1A] dark:text-white">
              {card.title}
            </h3>
          </div>
          {action}
        </div>
        {children}
      </div>

      <button
        type="button"
        onClick={() => onPrompt(card)}
        className="absolute bottom-3.5 right-3.5 z-40 inline-flex items-center gap-2 rounded-full bg-white/92 px-3 py-1.5 text-[11px] font-semibold text-[#1A1A1A] shadow-[0_12px_30px_rgba(20,21,24,0.08)] transition hover:bg-white dark:bg-[#202227] dark:text-white"
        aria-label={`Start chat for ${card.title}`}
        title={`Start chat for ${card.title}`}
      >
        <Newspaper className="size-3.5" />
        Ask Velion
      </button>
    </div>
  );
}

function MetricPill({
  icon,
  label,
  value,
}: {
  icon: ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="rounded-[16px] bg-white/62 px-3 py-2.5 shadow-sm">
      <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-[0.12em] text-[#7F97A7]">
        {icon}
        <span>{label}</span>
      </div>
      <p className="mt-0.5 text-[11px] font-semibold text-[#163649]">{value}</p>
    </div>
  );
}

function InformationSkeleton({ lines }: { lines: number }) {
  return (
    <div className="space-y-2">
      {Array.from({ length: lines }).map((_, index) => (
        <div key={index} className="h-10 rounded-[14px] bg-white/58" />
      ))}
    </div>
  );
}

function InformationError({ text }: { text: string }) {
  return (
    <div className="rounded-[18px] bg-[#FFF5F3] px-3 py-2.5 text-[12px] font-medium text-[#9F4535] dark:bg-[#261616] dark:text-[#FFB7AC]">
      {text}
    </div>
  );
}
