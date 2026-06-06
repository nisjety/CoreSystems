import "server-only";
import type { ControlPlaneContextValue } from "@/lib/control-plane/context-types";
import { isReadIntegrationUnavailable } from "@/lib/integrations/optional-service";
import { listNovuNotifications } from "@/lib/integrations/novu";
import { RequestActorError, requireRequestActor, type RequestActor } from "@/lib/integrations/request-actor";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";
import {
  emptyCalendar,
  emptyNotifications,
  type NavbarPayload,
  type NavbarProfile,
  type ThemePayload,
} from "@/features/shell-v2/lib/navbar-types";

type AppearanceSettings = {
  theme?: "light" | "dark" | "system" | "auto";
  colorScheme?: string | null;
};

function normalizeTheme(theme: AppearanceSettings["theme"]): ThemePayload["theme"] {
  return theme === "auto" ? "system" : theme ?? "system";
}

function normalizeColorScheme(value: unknown) {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

function profileFromActor(actor: RequestActor): NavbarProfile {
  return {
    id: actor.userId,
    name: actor.name?.trim() || actor.email?.split("@")[0] || "Account",
    email: actor.email,
    avatar: actor.avatar,
    status: "online",
  };
}

export function buildNavbarSeedFromControlPlaneContext(
  context: Pick<ControlPlaneContextValue, "appearance" | "user">,
): NavbarPayload {
  return {
    calendar: emptyCalendar,
    notifications: emptyNotifications,
    profile: context.user
      ? {
          id: context.user.id,
          name: context.user.name?.trim() || context.user.email?.split("@")[0] || "Account",
          email: context.user.email,
          avatar: context.user.image ?? undefined,
          status: "online",
        }
      : null,
    theme: context.appearance
      ? {
          colorScheme: normalizeColorScheme(context.appearance.colorScheme),
          theme: context.appearance.theme,
        }
      : null,
  };
}

export async function loadNavbarPayload(): Promise<NavbarPayload> {
  const actor = await requireRequestActor();
  const profile = profileFromActor(actor);

  const [notificationsResult, calendarResult, themeResult] = await Promise.allSettled([
    listNovuNotifications(actor, 40),
    fetchUserCoreJson<typeof emptyCalendar>(actor, "/api/v1/calendar/events"),
    fetchUserCoreJson<AppearanceSettings>(actor, "/api/v1/settings/appearance"),
  ]);

  const notifications = notificationsResult.status === "fulfilled"
    ? notificationsResult.value
    : emptyNotifications;

  const calendar = calendarResult.status === "fulfilled"
    ? calendarResult.value
    : emptyCalendar;

  let theme: ThemePayload | null = null;
  if (themeResult.status === "fulfilled") {
    theme = {
      colorScheme: normalizeColorScheme(themeResult.value.colorScheme),
      theme: normalizeTheme(themeResult.value.theme),
    };
  } else if (isReadIntegrationUnavailable(themeResult.reason)) {
    theme = { configured: false, colorScheme: null, theme: "system" };
  }

  return {
    calendar,
    notifications,
    profile,
    theme,
  };
}

export function resolveNavbarPayloadError(error: unknown) {
  if (error instanceof RequestActorError || error instanceof UserCoreError) {
    return {
      code: error.code,
      message: error.message,
      status: error.status,
    };
  }

  return {
    code: "navbar_load_failed",
    message: "Navbar data could not be loaded.",
    status: 500,
  };
}
