"use client";

import Link from "next/link";
import type { Route } from "next";
import { useRouter } from "next/navigation";
import { useEffect, useReducer, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  CircleDot,
  Eye,
  EyeOff,
  Fingerprint,
  Globe,
  KeyRound,
  ListCheck,
  Lock,
  LogIn,
  Mail,
  ShieldCheck,
  User,
  UserPlus,
  type LucideIcon,
} from "lucide-react";
import { authCopy } from "@/features/auth/lib/auth-copy";
import {
  authModeDescription,
  authModeTitle,
  calculatePasswordStrength,
  signInSchema,
  signUpSchema,
  type AuthMode,
} from "@/features/auth/lib/auth-schema";
import { authClient } from "@/lib/auth/auth-client";
import { cn } from "@/lib/utils";

type AuthStatus =
  | { type: "idle" }
  | { type: "loading"; message: string }
  | { type: "error"; message: string }
  | { type: "two-factor"; email: string }
  | { type: "success"; message: string };

type AuthValues = {
  confirmPassword: string;
  email: string;
  name: string;
  password: string;
  twoFactorCode: string;
};

type AuthPageState = {
  cookieConsent: "accepted" | "rejected" | null;
  cookieSettingsOpen: boolean;
  languageOpen: boolean;
  locale: "NO" | "EN";
  mode: AuthMode;
  showPassword: boolean;
  status: AuthStatus;
  values: AuthValues;
};

type AuthPageAction =
  | { type: "choose-locale"; locale: "NO" | "EN" }
  | { type: "open-cookie-settings" }
  | { type: "remember-cookie-consent"; choice: "accepted" | "rejected" }
  | { type: "set-cookie-settings-open"; open: boolean }
  | { type: "set-language-open"; open: boolean }
  | { type: "set-mode"; mode: AuthMode }
  | { type: "set-status"; status: AuthStatus }
  | { type: "toggle-password" }
  | { type: "update-value"; field: keyof AuthValues; value: string };

function createEmptyAuthValues(): AuthValues {
  return {
    name: "",
    email: "",
    password: "",
    confirmPassword: "",
    twoFactorCode: "",
  };
}

const passwordLabels = {
  minLength: "12+ tegn",
  uppercase: "stor bokstav",
  lowercase: "liten bokstav",
  number: "tall",
  special: "symbol",
  long: "16+ tegn",
} as const;

function createInitialAuthPageState(mode: AuthMode): AuthPageState {
  return {
    cookieConsent: null,
    cookieSettingsOpen: false,
    languageOpen: false,
    locale: "NO",
    mode,
    showPassword: false,
    status: { type: "idle" },
    values: createEmptyAuthValues(),
  };
}

function authPageReducer(state: AuthPageState, action: AuthPageAction): AuthPageState {
  switch (action.type) {
    case "choose-locale":
      return {
        ...state,
        languageOpen: false,
        locale: action.locale,
        status: {
          type: "success",
          message: action.locale === "NO" ? "Språk satt til norsk." : "Language set to English.",
        },
      };
    case "open-cookie-settings":
      return {
        ...state,
        cookieConsent: null,
        cookieSettingsOpen: true,
      };
    case "remember-cookie-consent":
      return {
        ...state,
        cookieConsent: action.choice,
        cookieSettingsOpen: false,
      };
    case "set-cookie-settings-open":
      return {
        ...state,
        cookieSettingsOpen: action.open,
      };
    case "set-language-open":
      return {
        ...state,
        languageOpen: action.open,
      };
    case "set-mode":
      return {
        ...state,
        mode: action.mode,
        status: { type: "idle" },
        values: createEmptyAuthValues(),
      };
    case "set-status":
      return {
        ...state,
        status: action.status,
      };
    case "toggle-password":
      return {
        ...state,
        showPassword: !state.showPassword,
      };
    case "update-value":
      return {
        ...state,
        status: state.status.type === "error" ? { type: "idle" } : state.status,
        values: {
          ...state.values,
          [action.field]: action.value,
        },
      };
  }
}

function toCamelCaseText(value: string) {
  return value
    .toLowerCase()
    .replace(/(^|\s)\S/g, (character) => character.toUpperCase());
}

function getAuthErrorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    const message = String(error.message);
    return message || "Innlogging feilet.";
  }

  return "Innlogging feilet.";
}

function normalizeCallbackUrl(value: string | null) {
  if (!value || !value.startsWith("/") || value.startsWith("//") || value.startsWith("/login")) {
    return "/dashboard";
  }

  return value;
}

export function VelionAuthPage({
  callbackUrl: callbackUrlInput,
  initialMode = "signin",
}: {
  callbackUrl?: string;
  initialMode?: AuthMode;
}) {
  const { push } = useRouter();
  const callbackUrl = normalizeCallbackUrl(callbackUrlInput ?? null);
  const [state, dispatch] = useReducer(authPageReducer, initialMode, createInitialAuthPageState);
  const {
    cookieConsent,
    cookieSettingsOpen,
    languageOpen,
    locale,
    mode,
    showPassword,
    status,
    values,
  } = state;
  const [viewportHeight, setViewportHeight] = useState<number | null>(null);
  const [contentHeight, setContentHeight] = useState<number | undefined>(undefined);
  const cardRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);

  const strength = calculatePasswordStrength(values.password);

  useEffect(() => {
    const updateViewportHeight = () => {
      setViewportHeight(window.innerHeight);
    };

    updateViewportHeight();
    window.addEventListener("resize", updateViewportHeight);
    return () => {
      window.removeEventListener("resize", updateViewportHeight);
    };
  }, []);

  useEffect(() => {
    const node = contentRef.current;
    if (!node) {
      return;
    }

    const measureContent = () => {
      const nextHeight = node.scrollHeight;
      setContentHeight((currentHeight) => currentHeight === nextHeight ? currentHeight : nextHeight);
    };
    const frame = window.requestAnimationFrame(measureContent);
    const observer =
      typeof ResizeObserver !== "undefined" ? new ResizeObserver(measureContent) : null;
    observer?.observe(node);
    const mutationObserver =
      typeof MutationObserver !== "undefined" ? new MutationObserver(measureContent) : null;
    mutationObserver?.observe(node, { childList: true, subtree: true, characterData: true });

    return () => {
      window.cancelAnimationFrame(frame);
      observer?.disconnect();
      mutationObserver?.disconnect();
    };
  }, []);

  const updateValue = (field: keyof typeof values, value: string) => {
    dispatch({ type: "update-value", field, value });
  };

  const switchMode = (nextMode: AuthMode) => {
    dispatch({ type: "set-mode", mode: nextMode });
  };

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();

    dispatch({ type: "set-status", status: { type: "loading", message: "Åpner sikker økt…" } });

    try {
      if (mode === "signup") {
        const parsed = signUpSchema.safeParse({
          name: values.name,
          email: values.email,
          password: values.password,
          confirmPassword: values.confirmPassword,
        });

        if (!parsed.success) {
          dispatch({
            type: "set-status",
            status: {
              type: "error",
              message: parsed.error.issues[0]?.message ?? "Kontroller registreringsskjemaet.",
            },
          });
          return;
        }

        const account = await authClient.signUp.email({
          email: parsed.data.email,
          password: parsed.data.password,
          name: parsed.data.name,
          callbackURL: "/onboarding",
        });

        if (account.error) {
          dispatch({ type: "set-status", status: { type: "error", message: getAuthErrorMessage(account.error) } });
          return;
        }

        const newSession = await authClient.signIn.email({
          email: parsed.data.email,
          password: parsed.data.password,
          callbackURL: "/onboarding",
        });

        if (newSession.error) {
          dispatch({ type: "set-mode", mode: "signin" });
          dispatch({
            type: "set-status",
            status: {
              type: "success",
              message: "Konto opprettet. Logg inn for å fortsette.",
            },
          });
          return;
        }

        dispatch({ type: "set-status", status: { type: "success", message: "Konto opprettet. Åpner onboarding." } });
        push("/onboarding");
        return;
      }

      const parsed = signInSchema.safeParse({
        email: values.email,
        password: values.password,
      });

      if (!parsed.success) {
        dispatch({
          type: "set-status",
          status: {
            type: "error",
            message: parsed.error.issues[0]?.message ?? "Kontroller innloggingsskjemaet.",
          },
        });
        return;
      }

      const session = await authClient.signIn.email({
        email: parsed.data.email,
        password: parsed.data.password,
        callbackURL: callbackUrl,
      });
      const sessionData = session.data as { twoFactorRedirect?: boolean } | null;

      if (session.error) {
        dispatch({ type: "set-status", status: { type: "error", message: getAuthErrorMessage(session.error) } });
        return;
      }

      if (sessionData?.twoFactorRedirect) {
        dispatch({ type: "set-status", status: { type: "two-factor", email: parsed.data.email } });
        return;
      }

      dispatch({ type: "set-status", status: { type: "success", message: "Økt bekreftet. Åpner arbeidsflate." } });
      push(callbackUrl as Route);
    } catch (error) {
      dispatch({ type: "set-status", status: { type: "error", message: getAuthErrorMessage(error) } });
    }
  };

  const verifyTwoFactor = async (event: React.FormEvent) => {
    event.preventDefault();
    const code = values.twoFactorCode.replace(/\D/g, "");

    if (code.length < 6) {
      dispatch({ type: "set-status", status: { type: "error", message: "Skriv inn den sekssifrede koden." } });
      return;
    }

    dispatch({ type: "set-status", status: { type: "loading", message: "Bekrefter tofaktorkode…" } });

    try {
      const result = await authClient.twoFactor.verifyTotp({
        code,
        trustDevice: true,
      });

      if (result.error) {
        dispatch({ type: "set-status", status: { type: "error", message: getAuthErrorMessage(result.error) } });
        return;
      }

      dispatch({ type: "set-status", status: { type: "success", message: "Tofaktor bekreftet." } });
      push(callbackUrl as Route);
    } catch (error) {
      dispatch({ type: "set-status", status: { type: "error", message: getAuthErrorMessage(error) } });
    }
  };

  const beginSocialSignIn = async (provider: "google" | "microsoft") => {
    dispatch({
      type: "set-status",
      status: { type: "loading", message: `Åpner ${toCamelCaseText(provider)} innlogging…` },
    });

    try {
      const result = await authClient.signIn.social({
        provider,
        callbackURL: callbackUrl,
      });

      if (result.error) {
        dispatch({ type: "set-status", status: { type: "error", message: getAuthErrorMessage(result.error) } });
      }
    } catch (error) {
      dispatch({ type: "set-status", status: { type: "error", message: getAuthErrorMessage(error) } });
    }
  };

  const beginPasskeySignIn = async () => {
    dispatch({
      type: "set-status",
      status: { type: "loading", message: "Åpner passkey-innlogging…" },
    });

    try {
      const result = await authClient.signIn.passkey();

      if (result?.error) {
        dispatch({
          type: "set-status",
          status: { type: "error", message: getAuthErrorMessage(result.error) },
        });
        return;
      }

      window.location.href = callbackUrl;
    } catch (error) {
      dispatch({
        type: "set-status",
        status: { type: "error", message: getAuthErrorMessage(error) },
      });
    }
  };

  const chooseLocale = (nextLocale: "NO" | "EN") => {
    dispatch({ type: "choose-locale", locale: nextLocale });
  };

  const rememberCookieConsent = (choice: "accepted" | "rejected") => {
    document.cookie = `velion_cookie_consent=${choice}; path=/; max-age=31536000; SameSite=Lax`;
    dispatch({ type: "remember-cookie-consent", choice });
  };

  const openCookieSettings = () => {
    dispatch({ type: "open-cookie-settings" });
  };

  const busy = status.type === "loading";
  const title = authModeTitle(mode);
  const cardScale = viewportHeight
    ? Math.min(1, Math.max(0.52, (viewportHeight - 18) / 1080))
    : 1;
  const animatedContentHeight = contentHeight
    ? Math.max(0, contentHeight + (mode === "signin" ? 6 : mode === "signup" ? -18 : 0))
    : undefined;

  return (
    <div
      className="auth-grain relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 opacity-100 transition-opacity duration-800 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10"
      style={
        {
          "--primary": "#111111",
          "--primary-foreground": "#ffffff",
          "--ring": "#111111",
        } as React.CSSProperties
      }
    >
      <div
        ref={cardRef}
        className="relative z-[120] grid w-full max-w-[70.5rem] overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr] xl:max-w-[72rem]"
        style={{
          transform: `scale(${cardScale})`,
          transformOrigin: "center center",
        }}
      >
        <AuthFormPane
          animatedContentHeight={animatedContentHeight}
          busy={busy}
          contentRef={contentRef}
          languageOpen={languageOpen}
          locale={locale}
          mode={mode}
          showPassword={showPassword}
          status={status}
          strength={strength}
          title={title}
          values={values}
          onChooseLocale={chooseLocale}
          onLanguageOpenChange={(open) => dispatch({ type: "set-language-open", open })}
          onModeChange={switchMode}
          onPasskeySignIn={beginPasskeySignIn}
          onSocialSignIn={beginSocialSignIn}
          onSubmit={submit}
          onTogglePassword={() => dispatch({ type: "toggle-password" })}
          onUpdateValue={updateValue}
          onVerifyTwoFactor={verifyTwoFactor}
        />

        <AuthVisualPane
          cookieConsent={cookieConsent}
          cookieSettingsOpen={cookieSettingsOpen}
          onAcceptCookies={() => rememberCookieConsent("accepted")}
          onRejectCookies={() => rememberCookieConsent("rejected")}
          onToggleCookieSettings={openCookieSettings}
        />
      </div>

      <AuthFooter onOpenCookieSettings={openCookieSettings} />
      <AuthBrandButton />
      <CookiePreferencesDialog
        open={cookieSettingsOpen}
        onClose={() => dispatch({ type: "set-cookie-settings-open", open: false })}
        onAccept={rememberCookieConsent}
      />
    </div>
  );
}

function AuthFooter({ onOpenCookieSettings }: { onOpenCookieSettings: () => void }) {
  return (
    <div className="pointer-events-none absolute bottom-3 left-0 right-0 hidden text-center md:block">
      <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-6 text-xs tracking-[0.02em] text-[#6A655F]">
        {authCopy.footer.map((item) => (
          <button
            key={item}
            type="button"
            onClick={item === "Cookie-innstillinger" ? onOpenCookieSettings : undefined}
            className="transition-colors hover:text-[#1C1C1C]"
          >
            {item}
          </button>
        ))}
      </div>
    </div>
  );
}

function AuthBrandButton() {
  return (
    <button
      type="button"
      aria-label="Velion"
      className="absolute bottom-4 left-5 hidden size-11 items-center justify-center rounded-full border border-white/20 bg-[#202020] text-[20px] text-white shadow-[0_10px_26px_rgba(0,0,0,0.22)] md:flex"
    >
      N
    </button>
  );
}

function AuthFormPane({
  animatedContentHeight,
  busy,
  contentRef,
  languageOpen,
  locale,
  mode,
  showPassword,
  status,
  strength,
  title,
  values,
  onChooseLocale,
  onLanguageOpenChange,
  onModeChange,
  onPasskeySignIn,
  onSocialSignIn,
  onSubmit,
  onTogglePassword,
  onUpdateValue,
  onVerifyTwoFactor,
}: {
  animatedContentHeight: number | undefined;
  busy: boolean;
  contentRef: React.RefObject<HTMLDivElement | null>;
  languageOpen: boolean;
  locale: "NO" | "EN";
  mode: AuthMode;
  showPassword: boolean;
  status: AuthStatus;
  strength: ReturnType<typeof calculatePasswordStrength>;
  title: string;
  values: AuthValues;
  onChooseLocale: (locale: "NO" | "EN") => void;
  onLanguageOpenChange: (open: boolean) => void;
  onModeChange: (mode: AuthMode) => void;
  onPasskeySignIn: () => void;
  onSocialSignIn: (provider: "google" | "microsoft") => void;
  onSubmit: (event: React.FormEvent) => void;
  onTogglePassword: () => void;
  onUpdateValue: (field: keyof AuthValues, value: string) => void;
  onVerifyTwoFactor: (event: React.FormEvent) => void;
}) {
  return (
    <section className="flex items-center justify-center rounded-[24px] bg-white px-5 py-6 sm:p-7 md:rounded-l-[24px] md:rounded-r-none md:p-8 lg:px-10 lg:py-9 xl:px-16 xl:py-10">
      <div className="w-full max-w-[21rem] sm:max-w-[22rem] lg:max-w-[22.75rem] xl:max-w-[25rem]">
        <div className="mb-4 flex items-center justify-between xl:mb-6">
          <Link
            href="/"
            className="text-xs font-bold tracking-tight text-[#111111] transition-colors hover:text-[#FF2E63]"
          >
            {authCopy.back}
          </Link>
          <AuthLanguageSwitcher
            locale={locale}
            open={languageOpen}
            onOpenChange={onLanguageOpenChange}
            onChoose={onChooseLocale}
          />
        </div>

        <AuthTabs mode={mode} disabled={busy} onChange={onModeChange} />

        <div className="mb-4 text-left xl:mb-5">
          <h1 className="font-inter text-[clamp(48px,5.1vw,76px)] font-[450] leading-none tracking-normal text-[#1C1C1C]">
            {toCamelCaseText(title)}
          </h1>
          <p className="mt-3 text-[15px] leading-[1.6] text-[#66615B] xl:mt-4">
            {authModeDescription(mode)}
          </p>
        </div>

        <div
          className="relative overflow-hidden transition-[height] duration-[520ms] ease-in-out"
          style={{ height: animatedContentHeight ? `${animatedContentHeight}px` : undefined }}
        >
          <div ref={contentRef} className="transform-gpu">
            {status.type === "two-factor" ? (
              <TwoFactorForm
                code={values.twoFactorCode}
                email={status.email}
                onCodeChange={(value) => onUpdateValue("twoFactorCode", value)}
                onSubmit={onVerifyTwoFactor}
              />
            ) : (
              <form onSubmit={onSubmit} className="space-y-4">
                {mode === "signup" ? (
                  <AuthField
                    id="name"
                    label="Navn *"
                    icon={User}
                    value={values.name}
                    autoComplete="name"
                    placeholder="Ima Fernandes"
                    onChange={(value) => onUpdateValue("name", value)}
                  />
                ) : null}

                <AuthField
                  id="email"
                  label="E-postadresse *"
                  icon={Mail}
                  type="email"
                  value={values.email}
                  autoComplete="email webauthn"
                  placeholder="navn@eksempel.no"
                  onChange={(value) => onUpdateValue("email", value)}
                />

                <AuthField
                  id="password"
                  label="Passord *"
                  icon={Lock}
                  type={showPassword ? "text" : "password"}
                  value={values.password}
                  autoComplete={mode === "signin" ? "current-password" : "new-password"}
                  placeholder="••••••••"
                  onChange={(value) => onUpdateValue("password", value)}
                  rightSlot={
                    <button
                      type="button"
                      onClick={onTogglePassword}
                      className="rounded p-1 text-[#8A8D96] transition-colors hover:text-[#1C1C1C]"
                      aria-label={showPassword ? "Skjul passord" : "Vis passord"}
                    >
                      {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                    </button>
                  }
                />

                {mode === "signup" ? (
                  <>
                    <PasswordStrengthBar score={strength.score} missing={strength.missing} />
                    <AuthField
                      id="confirmPassword"
                      label="Bekreft passord *"
                      icon={Lock}
                      type="password"
                      value={values.confirmPassword}
                      autoComplete="new-password"
                      placeholder="••••••••"
                      onChange={(value) => onUpdateValue("confirmPassword", value)}
                    />
                  </>
                ) : null}

                <button
                  type="submit"
                  disabled={busy}
                  className="inline-flex h-11 w-full items-center justify-center rounded-[10px] bg-[#111111] px-5 text-[15px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                >
                  {status.type === "loading" ? status.message : mode === "signup" ? "Registrer" : "Logg inn"}
                </button>
              </form>
            )}

            <AuthStatusLine status={status} />

            <SocialProviderRow
              busy={busy}
              onMicrosoft={() => onSocialSignIn("microsoft")}
              onGoogle={() => onSocialSignIn("google")}
            />

            <button
              type="button"
              disabled={busy}
              onClick={onPasskeySignIn}
              className="mt-4 inline-flex h-10 w-full items-center justify-center gap-2 rounded-lg border border-[#D6D2CB] bg-white text-sm font-medium text-[#1C1C1C] transition-colors hover:border-[#A09890] hover:bg-[#FAFAFA] disabled:opacity-50"
            >
              <KeyRound className="size-4" />
              Bruk passkey
            </button>

            <div className="mt-3 text-left">
              <p className="text-xs leading-[1.65] tracking-[0.02em] text-[#6A655F]">
                {mode === "signin" ? (
                  <>
                    {authCopy.termsPrefixSignin}{" "}
                    <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C]">
                      {authCopy.termsUser}
                    </button>{" "}
                    {authCopy.termsAnd}{" "}
                    <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C]">
                      {authCopy.termsPrivacy}
                    </button>
                    .
                  </>
                ) : (
                  <>
                    {authCopy.signupDataNotice}{" "}
                    <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C]">
                      {authCopy.deleteCookie}
                    </button>
                    .
                  </>
                )}
              </p>
            </div>
          </div>
        </div>

        <div className="mt-7 hidden text-left sm:block xl:mt-8">
          <p className="text-xs tracking-[0.02em] text-[#6A655F]">
            {authCopy.supportNeedHelp}{" "}
            <a href={`mailto:${authCopy.supportEmail}`} className="text-[#3E3A35] transition-colors hover:text-[#1C1C1C]">
              {authCopy.supportContact}
            </a>
          </p>
        </div>
      </div>
    </section>
  );
}

function AuthLanguageSwitcher({
  locale,
  open,
  onOpenChange,
  onChoose,
}: {
  locale: "NO" | "EN";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChoose: (locale: "NO" | "EN") => void;
}) {
  return (
    <div className="relative flex items-center">
      <button
        type="button"
        aria-label="Select language"
        aria-expanded={open}
        aria-haspopup="true"
        onClick={() => onOpenChange(!open)}
        className="inline-flex items-center gap-2 rounded-lg bg-background/80 p-2 text-sm font-medium text-muted-foreground shadow-sm backdrop-blur-sm transition-colors hover:text-foreground hover:shadow-md focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
      >
        <Globe className="size-4" />
        <ChevronDown className={cn("size-4 transition-transform", open ? "rotate-180" : "rotate-0")} />
      </button>
      <span className="ml-2 inline-block size-1.5 rounded-full bg-[#FF2E63]/70" aria-hidden="true" />
      {open ? (
        <div className="absolute right-5 top-10 z-[300] min-w-[140px] rounded-lg border border-border bg-background py-2 shadow-lg">
          {([
            { code: "NO", name: "Norsk", flag: "🇳🇴" },
            { code: "EN", name: "English", flag: "🇺🇸" },
          ] as const).map((language) => {
            const selected = locale === language.code;
            return (
              <button
                key={language.code}
                type="button"
                onClick={() => onChoose(language.code)}
                className={cn(
                  "flex w-full items-center gap-3 px-4 py-3 text-base transition-colors hover:bg-muted",
                  selected ? "text-foreground" : "text-muted-foreground",
                )}
              >
                <span className="text-lg">{language.flag}</span>
                <span className="flex-1 text-left">{language.name}</span>
                {selected ? <Check className="size-4" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

function AuthTabs({
  mode,
  disabled,
  onChange,
}: {
  mode: AuthMode;
  disabled: boolean;
  onChange: (mode: AuthMode) => void;
}) {
  return (
    <div className="mb-6 mt-0.5 sm:mb-8" role="tablist" aria-label="Autentisering">
      <div className="inline-flex rounded-full bg-[#F0EFED] p-1 shadow-[inset_0_1px_1px_rgba(0,0,0,0.04),0_6px_16px_rgba(0,0,0,0.06)]">
        {([
          { id: "signin", label: "Logg Inn", icon: LogIn },
          { id: "signup", label: "Registrer", icon: UserPlus },
        ] as const).map((candidate) => {
          const active = mode === candidate.id;
          const Icon = candidate.icon;
          return (
            <button
              key={candidate.id}
              type="button"
              role="tab"
              aria-selected={active}
              disabled={disabled}
              onClick={() => onChange(candidate.id)}
              className={cn(
                "group relative inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs transition-all duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/20 sm:text-sm",
                active
                  ? "bg-white text-[#1C1C1E] shadow ring-1 ring-[#D6D2CB]"
                  : "text-[#777169] hover:bg-white/70 hover:text-[#1C1C1E]",
              )}
            >
              <Icon className="size-3.5" strokeWidth={1.8} />
              {candidate.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function AuthField({
  id,
  label,
  icon: Icon,
  value,
  onChange,
  type = "text",
  autoComplete,
  inputMode,
  placeholder,
  rightSlot,
}: {
  id: string;
  label: string;
  icon: LucideIcon;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  autoComplete?: string;
  inputMode?: React.HTMLAttributes<HTMLInputElement>["inputMode"];
  placeholder?: string;
  rightSlot?: React.ReactNode;
}) {
  return (
    <label className="block" htmlFor={id}>
      <span className="mb-1 block text-sm font-medium text-[#1C1C1C]">{label}</span>
      <span className="relative flex items-center">
        <Icon className="absolute left-3 size-4 text-[#8A8D96]" />
        <input
          id={id}
          name={id}
          type={type}
          aria-label={label}
          required
          value={value}
          autoComplete={autoComplete}
          inputMode={inputMode}
          placeholder={placeholder}
          onChange={(event) => onChange(event.target.value)}
          className="h-10 w-full rounded-lg border border-[#D6D2CB] bg-white py-2 pl-10 pr-10 text-sm text-[#1C1C1C] placeholder:text-[#A09890] transition-colors focus:border-[#111111] focus:outline-none focus:ring-2 focus:ring-[#111111]/10"
        />
        {rightSlot ? <span className="absolute right-2">{rightSlot}</span> : null}
      </span>
    </label>
  );
}

function PasswordStrengthBar({
  score,
  missing,
}: {
  score: number;
  missing: Array<keyof typeof passwordLabels>;
}) {
  const filled = Math.min(5, Math.max(0, score));
  return (
    <div className="space-y-2">
      <div className="flex gap-1">
        {Array.from({ length: 5 }).map((_, index) => (
          <span
            key={index}
            className={cn(
              "h-1 flex-1 rounded-full",
              index < filled ? "bg-[#111111]" : "bg-[#E5DFD3]",
            )}
          />
        ))}
      </div>
      {missing.length > 0 ? (
        <p className="text-xs text-[#6A655F]">
          Legg til {missing.slice(0, 3).map((key) => passwordLabels[key]).join(", ")}.
        </p>
      ) : (
        <p className="text-xs text-[#107A55]">Sterkt passord.</p>
      )}
    </div>
  );
}

function TwoFactorForm({
  code,
  email,
  onCodeChange,
  onSubmit,
}: {
  code: string;
  email: string;
  onCodeChange: (value: string) => void;
  onSubmit: (event: React.FormEvent) => void;
}) {
  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="rounded-xl border border-[#E5DFD3] bg-[#F7F4ED] p-4">
        <div className="flex items-start gap-3">
          <span className="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white">
            <KeyRound className="size-4" />
          </span>
          <div>
            <h2 className="text-sm font-semibold text-[#1C1C1C]">{authCopy.twoFactor.title}</h2>
            <p className="mt-1 text-xs leading-5 text-[#6A655F]">
              {authCopy.twoFactor.description} Sendt til {email}.
            </p>
          </div>
        </div>
      </div>
      <AuthField
        id="twoFactorCode"
        label={authCopy.twoFactor.codeLabel}
        icon={KeyRound}
        value={code}
        inputMode="numeric"
        placeholder="123456"
        onChange={(value) => onCodeChange(value.replace(/\D/g, "").slice(0, 6))}
      />
      <button
        type="submit"
        className="inline-flex w-full items-center justify-center rounded-md bg-[#111111] px-5 py-3 text-[11px] font-semibold uppercase tracking-[0.22em] text-white transition-opacity hover:opacity-80"
      >
        {authCopy.twoFactor.continue}
      </button>
      <button type="button" className="text-[11px] uppercase tracking-[0.18em] text-[#A09890] hover:text-[#111111]">
        {authCopy.twoFactor.backupLabel}
      </button>
    </form>
  );
}

function AuthStatusLine({ status }: { status: AuthStatus }) {
  if (status.type === "idle" || status.type === "two-factor") {
    return null;
  }
  const tone =
    status.type === "error"
      ? "text-[#B42318]"
      : status.type === "success"
        ? "text-[#107A55]"
        : "text-[#6A655F]";
  return <p className={cn("mt-3 text-xs leading-5", tone)}>{status.message}</p>;
}

function SocialProviderRow({
  busy,
  onMicrosoft,
  onGoogle,
}: {
  busy: boolean;
  onMicrosoft: () => void;
  onGoogle: () => void;
}) {
  return (
    <div className="mt-5">
      <div className="relative flex items-center justify-center">
        <div className="h-px flex-1 bg-[#E5E1DA]" />
        <span className="mx-3 bg-white px-2 text-[13px] text-[#7B756E]">eller med</span>
        <div className="h-px flex-1 bg-[#E5E1DA]" />
      </div>
      <div className="mt-4 flex items-center justify-center gap-14">
        <SocialCircleButton label={authCopy.social.microsoft} disabled={busy} onClick={onMicrosoft}>
          <MicrosoftGlyph />
        </SocialCircleButton>
        <SocialCircleButton label={authCopy.social.google} disabled={busy} onClick={onGoogle}>
          <GoogleGlyph />
        </SocialCircleButton>
        <SocialCircleButton label="SSO" disabled={busy} onClick={onMicrosoft}>
          <CircleDot className="size-5 text-[#A6ABB4]" />
        </SocialCircleButton>
        <SocialCircleButton label="Mer" disabled={busy} onClick={onGoogle}>
          <span className="text-[18px] leading-none text-[#A6ABB4]">⌣</span>
        </SocialCircleButton>
      </div>
    </div>
  );
}

function SocialCircleButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="grid size-10 place-items-center rounded-full border border-[#E3DFD8] bg-white shadow-sm transition-transform hover:-translate-y-0.5 hover:border-[#CFC8BC] disabled:opacity-50"
    >
      {children}
    </button>
  );
}

function MicrosoftGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="size-5">
      <path fill="#F25022" d="M2 2h9.2v9.2H2z" />
      <path fill="#7FBA00" d="M12.8 2H22v9.2h-9.2z" />
      <path fill="#00A4EF" d="M2 12.8h9.2V22H2z" />
      <path fill="#FFB900" d="M12.8 12.8H22V22h-9.2z" />
    </svg>
  );
}

function GoogleGlyph() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="size-5">
      <path fill="#4285F4" d="M21.6 12.23c0-.78-.07-1.53-.2-2.23H12v4.22h5.38a4.6 4.6 0 0 1-1.99 3.02v2.51h3.23c1.89-1.74 2.98-4.3 2.98-7.52z" />
      <path fill="#34A853" d="M12 22c2.7 0 4.97-.9 6.62-2.45l-3.23-2.51c-.9.6-2.04.95-3.39.95-2.6 0-4.8-1.75-5.59-4.12H3.07v2.59A9.99 9.99 0 0 0 12 22z" />
      <path fill="#FBBC05" d="M6.41 13.87A6.02 6.02 0 0 1 6.1 12c0-.65.11-1.28.31-1.87V7.54H3.07A9.99 9.99 0 0 0 2 12c0 1.61.39 3.13 1.07 4.46z" />
      <path fill="#EA4335" d="M12 6.01c1.47 0 2.78.5 3.82 1.5l2.87-2.87C16.96 3.01 14.69 2 12 2a9.99 9.99 0 0 0-8.93 5.54l3.34 2.59C7.2 7.76 9.4 6.01 12 6.01z" />
    </svg>
  );
}

function AuthVisualPane({
  cookieConsent,
  cookieSettingsOpen,
  onAcceptCookies,
  onRejectCookies,
  onToggleCookieSettings,
}: {
  cookieConsent: "accepted" | "rejected" | null;
  cookieSettingsOpen: boolean;
  onAcceptCookies: () => void;
  onRejectCookies: () => void;
  onToggleCookieSettings: () => void;
}) {
  return (
    <aside className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] md:block lg:min-h-[600px] xl:min-h-[640px]">
      <div
        className="absolute inset-0 bg-cover bg-center"
        style={{ backgroundImage: "url('/imagens/curved-interior-sculpture.png')" }}
      />
      <div className="pointer-events-none absolute bottom-8 left-5 top-8 z-20">
        <div className="absolute inset-y-0 left-0 w-px bg-[#FF2E63]/90" />
        <div className="absolute inset-y-0 -left-[3px] w-[8px] bg-[#FF3B5C]/35 blur-[7px]" />
        <div className="scanner-dot absolute -left-[4px] top-0 h-[20px] w-[9px] rounded-lg bg-gradient-to-b from-[#FF3B5C]/15 via-[#FF3B5C]/40 to-[#FF3B5C]/15 shadow-[0_0_8px_rgba(255,59,92,0.5),0_0_16px_rgba(255,59,92,0.3),0_0_32px_rgba(255,59,92,0.15)]" />
      </div>
      <div className="absolute left-4 top-1/2 z-30 -translate-y-1/2 xl:left-5">
        <div className="flex flex-col items-center gap-10 text-white/90 lg:gap-11 xl:gap-12">
          <ShieldCheck strokeWidth={1.4} className="size-4 text-[#10B981] xl:size-5" />
          <Lock strokeWidth={1.4} className="size-4 xl:size-5" />
          <Fingerprint strokeWidth={1.4} className="size-4 text-[#FF2E63]/80 xl:size-5" />
        </div>
      </div>
      <CookieConsentCard
        consent={cookieConsent}
        settingsOpen={cookieSettingsOpen}
        onAccept={onAcceptCookies}
        onReject={onRejectCookies}
        onToggleSettings={onToggleCookieSettings}
      />
    </aside>
  );
}

function CookieConsentCard({
  consent,
  settingsOpen,
  onAccept,
  onReject,
  onToggleSettings,
}: {
  consent: "accepted" | "rejected" | null;
  settingsOpen: boolean;
  onAccept: () => void;
  onReject: () => void;
  onToggleSettings: () => void;
}) {
  if (consent && !settingsOpen) {
    return null;
  }

  return (
    <div className="velion-cookie-card absolute bottom-6 left-1/2 z-30 hidden w-[92%] -translate-x-1/2 lg:block">
      <div className="rounded-full bg-background px-3 py-2 shadow-lg">
        <div className="flex items-center gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-[#F2F3F5] text-[#6A6E78]">
            <CookieGlyph />
          </span>
          <p className="min-w-0 flex-1 text-sm leading-tight text-foreground/90">
            Ved å klikke «Godta», godtar du lagring av informasjonskapsler på enheten din.
          </p>
          <div className="flex shrink-0 items-center gap-2">
            <button
              type="button"
              aria-label="Cookie-innstillinger"
              title="Cookie-innstillinger"
              onClick={onToggleSettings}
              className="size-9 rounded-full bg-background ring-1 ring-border transition hover:bg-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <ListCheck className="mx-auto size-[18px] text-foreground/70" />
            </button>
            <button
              type="button"
              onClick={onReject}
              className="h-9 rounded-bl-[30px] rounded-br-[10px] rounded-tl-[30px] rounded-tr-[10px] bg-background px-4 text-foreground ring-1 ring-border transition hover:bg-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              Avvis
            </button>
            <button
              type="button"
              onClick={onAccept}
              className="h-9 rounded-bl-[10px] rounded-br-[30px] rounded-tl-[10px] rounded-tr-[30px] bg-primary px-5 font-medium text-primary-foreground transition hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              Godta
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function CookieGlyph() {
  return (
    <svg viewBox="0 0 24 24" className="size-[18px]" fill="currentColor" aria-hidden="true">
      <path d="M12 2a10 10 0 1010 10c0-.34-.02-.67-.06-.99a3 3 0 01-3.44-3.44C18.67 7.02 18.34 7 18 7a6 6 0 01-6-6zM8 12a1 1 0 110-2 1 1 0 010 2zm2 4a1 1 0 110-2 1 1 0 010 2zm5-3a1 1 0 110-2 1 1 0 010 2z" />
    </svg>
  );
}

function CookiePreferencesDialog({
  open,
  onClose,
  onAccept,
}: {
  open: boolean;
  onClose: () => void;
  onAccept: (choice: "accepted" | "rejected") => void;
}) {
  if (!open) {
    return null;
  }

  return (
    <dialog
      open
      aria-labelledby="cookie-prefs-title"
      className="fixed inset-0 z-[300] m-0 flex h-auto max-h-none w-auto max-w-none items-center justify-center border-0 bg-transparent p-0 text-inherit"
    >
      <button
        type="button"
        aria-label="Lukk"
        className="absolute inset-0 bg-background/60 backdrop-blur-sm"
        onClick={onClose}
      />
      <div className="relative max-h-[90svh] w-[92vw] max-w-xl overflow-y-auto rounded-2xl bg-card text-card-foreground shadow-2xl">
        <div className="grid grid-cols-3 items-center p-4">
          <button
            type="button"
            onClick={onClose}
            className="flex size-8 items-center justify-center rounded-full bg-muted text-foreground/80 transition hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label="Lukk"
          >
            ×
          </button>
          <h2 id="cookie-prefs-title" className="justify-self-center text-sm font-medium">
            Cookie-innstillinger
          </h2>
          <span className="justify-self-end" aria-hidden="true" />
        </div>
        <div className="border-b border-border p-4">
          <p className="text-sm text-muted-foreground">
            Velg hvordan Velion kan bruke informasjonskapsler på denne enheten.
          </p>
          <button
            type="button"
            onClick={() => onAccept("accepted")}
            className="mt-4 h-10 w-full rounded-full bg-primary font-medium text-primary-foreground transition hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            Tillat alle
          </button>
        </div>
        <div className="space-y-3 p-4">
          {["Nødvendige", "Preferanser", "Analyse"].map((label, index) => (
            <div key={label} className="flex items-center justify-between rounded-xl border border-border bg-background p-3">
              <div>
                <p className="text-sm font-medium">{label}</p>
                <p className="mt-1 text-xs text-muted-foreground">
                  {index === 0 ? "Kreves for sikker innlogging." : "Kan justeres senere."}
                </p>
              </div>
              <span
                className={cn(
                  "relative h-6 w-11 rounded-full ring-1 ring-border after:absolute after:left-0.5 after:top-0.5 after:size-5 after:rounded-full after:bg-background after:transition-transform",
                  index === 0 ? "bg-primary after:translate-x-5" : "bg-muted",
                )}
                aria-hidden="true"
              />
            </div>
          ))}
        </div>
        <div className="sticky bottom-0 flex justify-end gap-2 border-t border-border bg-card p-4">
          <button
            type="button"
            onClick={() => onAccept("rejected")}
            className="rounded-bl-[30px] rounded-br-[10px] rounded-tl-[30px] rounded-tr-[10px] bg-muted px-4 py-2 text-foreground transition hover:opacity-90"
          >
            Avvis alle
          </button>
          <button
            type="button"
            onClick={() => onAccept("accepted")}
            className="rounded-bl-[10px] rounded-br-[30px] rounded-tl-[10px] rounded-tr-[30px] bg-primary px-4 py-2 text-primary-foreground transition hover:bg-primary/90"
          >
            Lagre valg
          </button>
        </div>
      </div>
    </dialog>
  );
}
