import {
  requestBrowserLocation,
  type BrowserLocationCoordinates,
  type InformationDashboardPayload,
} from "@/features/dashboard-v2/lib/information-core";
import { apiGet } from "@/lib/api/client-envelope";

type DashboardInformationSnapshot = InformationDashboardPayload & {
  usingLiveLocation: boolean;
};

const DASHBOARD_INFORMATION_TTL_MS = 30 * 1000;

let cachedDashboardInformation:
  | {
      expiresAt: number;
      key: string;
      value: DashboardInformationSnapshot;
    }
  | null = null;
let pendingDashboardInformation:
  | {
      key: string;
      promise: Promise<DashboardInformationSnapshot>;
    }
  | null = null;

function buildDashboardInformationKey(
  coordinates: BrowserLocationCoordinates | null,
) {
  if (!coordinates) {
    return "dashboard-information:default";
  }

  return [
    "dashboard-information",
    coordinates.latitude.toFixed(4),
    coordinates.longitude.toFixed(4),
    typeof coordinates.altitude === "number"
      ? Math.round(coordinates.altitude).toString()
      : "na",
  ].join(":");
}

export async function loadDashboardInformationSnapshot() {
  const coordinates = await requestBrowserLocation();
  const key = buildDashboardInformationKey(coordinates);
  const now = Date.now();

  if (
    cachedDashboardInformation &&
    cachedDashboardInformation.key === key &&
    cachedDashboardInformation.expiresAt > now
  ) {
    return cachedDashboardInformation.value;
  }

  if (
    pendingDashboardInformation &&
    pendingDashboardInformation.key === key
  ) {
    return pendingDashboardInformation.promise;
  }

  const params = new URLSearchParams({
    newsLimit: "8",
    newsMaxAge: "24",
    trafficRadius: "40",
  });
  if (coordinates) {
    params.set("lat", coordinates.latitude.toString());
    params.set("lon", coordinates.longitude.toString());
    if (typeof coordinates.altitude === "number") {
      params.set("altitude", Math.round(coordinates.altitude).toString());
    }
  }

  const promise = apiGet<InformationDashboardPayload>(
    `/api/v1/information/dashboard?${params}`,
    { credentials: "include" },
  )
    .then((payload) => {
      const snapshot: DashboardInformationSnapshot = {
        ...payload,
        usingLiveLocation: Boolean(coordinates),
      };
      cachedDashboardInformation = {
        expiresAt: Date.now() + DASHBOARD_INFORMATION_TTL_MS,
        key,
        value: snapshot,
      };
      return snapshot;
    })
    .finally(() => {
      if (pendingDashboardInformation?.key === key) {
        pendingDashboardInformation = null;
      }
    });

  pendingDashboardInformation = {
    key,
    promise,
  };

  return promise;
}
