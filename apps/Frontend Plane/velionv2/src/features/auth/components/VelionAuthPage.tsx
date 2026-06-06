"use client";

import Link from "next/link";
import type { Route } from "next";
import { useRouter } from "next/navigation";
import {
  useEffect,
  useEffectEvent,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  Building2,
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
  X,
  type LucideIcon,
} from "lucide-react";

import { useAuthCopy, formatAuthText, type AuthLocale } from "@/features/auth/lib/auth-i18n";
import {
  calculatePasswordStrength,
  forgotSchema,
  resetSchema,
  signInSchema,
  signUpSchema,
  ssoSchema,
  type AuthMode,
  type PasswordStrengthKey,
} from "@/features/auth/lib/auth-schema";
import {
  beginSsoSignIn,
  detectWebAuthnSupport,
  isConditionalMediationAvailable,
  requestPasswordReset,
  resetPassword,
} from "@/features/auth/lib/auth-actions";
import { authClient } from "@/lib/auth/auth-client";
import { useLocale, type Locale } from "@/lib/i18n/locale-context";
import { cn } from "@/lib/utils";

type AuthCopy = ReturnType<typeof useAuthCopy>["copy"];

type AuthStatus =
  | { type: "idle" }
  | { type: "loading"; message: string }
  | { type: "error"; message: string }
  | { type: "two-factor"; email: string }
  | { type: "verify-email"; email: string }
  | { type: "success"; message: string };

type AuthValues = {
  confirmPassword: string;
  email: string;
  name: string;
  password: string;
  twoFactorCode: string;
  businessEmail: string;
  orgDomain: string;
};

type AuthPageState = {
  cookieConsent: "accepted" | "rejected" | null;
  cookieSettingsOpen: boolean;
  mode: AuthMode;
  showPassword: boolean;
  status: AuthStatus;
  values: AuthValues;
};

type AuthPageAction =
  | { type: "open-cookie-settings" }
  | { type: "remember-cookie-consent"; choice: "accepted" | "rejected" }
  | { type: "set-cookie-settings-open"; open: boolean }
  | { type: "set-mode"; mode: AuthMode }
  | { type: "set-status"; status: AuthStatus }
  | { type: "toggle-password" }
  | { type: "update-value"; field: keyof AuthValues; value: string };

function createEmptyValues(): AuthValues {
  return { name: "", email: "", password: "", confirmPassword: "", twoFactorCode: "", businessEmail: "", orgDomain: "" };
}

function createInitialState(mode: AuthMode): AuthPageState {
  return {
    cookieConsent: null,
    cookieSettingsOpen: false,
    mode,
    showPassword: false,
    status: { type: "idle" },
    values: createEmptyValues(),
  };
}

function subscribeToBrowserCapability() {
  return () => undefined;
}

function getPasskeySupportServerSnapshot() {
  return false;
}

function getViewportHeightServerSnapshot() {
  return null;
}

function subscribeToViewportHeight(onStoreChange: () => void) {
  if (typeof window === "undefined") {
    return () => undefined;
  }

  window.addEventListener("resize", onStoreChange);
  return () => window.removeEventListener("resize", onStoreChange);
}

function getViewportHeightSnapshot() {
  if (typeof window === "undefined") {
    return null;
  }

  return window.innerHeight;
}

function reducer(state: AuthPageState, action: AuthPageAction): AuthPageState {
  switch (action.type) {
    case "open-cookie-settings":
      return { ...state, cookieConsent: null, cookieSettingsOpen: true };
    case "remember-cookie-consent":
      return { ...state, cookieConsent: action.choice, cookieSettingsOpen: false };
    case "set-cookie-settings-open":
      return { ...state, cookieSettingsOpen: action.open };
    case "set-mode":
      return { ...state, mode: action.mode, status: { type: "idle" }, values: createEmptyValues() };
    case "set-status":
      return { ...state, status: action.status };
    case "toggle-password":
      return { ...state, showPassword: !state.showPassword };
    case "update-value":
      return {
        ...state,
        status: state.status.type === "error" ? { type: "idle" } : state.status,
        values: { ...state.values, [action.field]: action.value },
      };
  }
}

function toTitleCase(value: string) {
  return value.toLowerCase().replace(/(^|\s)\S/g, (c) => c.toUpperCase());
}

function getErrorMessage(error: unknown, fallback: string) {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message) || fallback;
  }
  return fallback;
}

function normalizeCallbackUrl(value: string | null) {
  if (!value || !value.startsWith("/") || value.startsWith("//") || value.startsWith("/login")) {
    return "/dashboard";
  }
  return value;
}

function toBrowserUrl(value: string) {
  if (typeof window === "undefined") return value;
  return new URL(value, window.location.origin).toString();
}

export function VelionAuthPage({
  callbackUrl: callbackUrlInput,
  initialMode = "signin",
  resetToken,
}: {
  callbackUrl?: string;
  initialMode?: AuthMode;
  resetToken?: string;
}) {
  const { push } = useRouter();
  const { copy, locale } = useAuthCopy();
  const callbackUrl = normalizeCallbackUrl(callbackUrlInput ?? null);
  const [state, dispatch] = useReducer(reducer, initialMode, createInitialState);
  const { cookieConsent, cookieSettingsOpen, mode, showPassword, status, values } = state;

  const viewportHeight = useSyncExternalStore(
    subscribeToViewportHeight,
    getViewportHeightSnapshot,
    getViewportHeightServerSnapshot,
  );
  const passkeySupported = useSyncExternalStore(
    subscribeToBrowserCapability,
    detectWebAuthnSupport,
    getPasskeySupportServerSnapshot,
  );

  const strength = calculatePasswordStrength(values.password);

  const updateValue = (field: keyof AuthValues, value: string) => dispatch({ type: "update-value", field, value });
  const switchMode = (next: AuthMode) => dispatch({ type: "set-mode", mode: next });

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    dispatch({ type: "set-status", status: { type: "loading", message: copy.status.opening } });
    try {
      if (mode === "forgot") return void (await submitForgot());
      if (mode === "reset") return void (await submitReset());
      if (mode === "sso") return void (await submitSso());

      if (mode === "signup") {
        const parsed = signUpSchema.safeParse({
          name: values.name,
          email: values.email,
          password: values.password,
          confirmPassword: values.confirmPassword,
        });
        if (!parsed.success) {
          return setError(parsed.error.issues[0]?.message ?? copy.status.checkForm);
        }
        const account = await authClient.signUp.email({
          email: parsed.data.email,
          password: parsed.data.password,
          name: parsed.data.name,
          callbackURL: toBrowserUrl("/onboarding"),
        });
        if (account.error) return setError(getErrorMessage(account.error, copy.status.genericError));

        const session = await authClient.signIn.email({
          email: parsed.data.email,
          password: parsed.data.password,
          callbackURL: toBrowserUrl("/onboarding"),
        });
        if (session.error) {
          // Most commonly: email verification required before sign-in.
          dispatch({ type: "set-status", status: { type: "verify-email", email: parsed.data.email } });
          return;
        }
        dispatch({ type: "set-status", status: { type: "success", message: copy.status.accountCreated } });
        push("/onboarding");
        return;
      }

      const parsed = signInSchema.safeParse({ email: values.email, password: values.password });
      if (!parsed.success) {
        return setError(parsed.error.issues[0]?.message ?? copy.status.checkForm);
      }
      const session = await authClient.signIn.email({
        email: parsed.data.email,
        password: parsed.data.password,
        callbackURL: toBrowserUrl(callbackUrl),
      });
      const data = session.data as { twoFactorRedirect?: boolean } | null;
      if (session.error) return setError(getErrorMessage(session.error, copy.status.genericError));
      if (data?.twoFactorRedirect) {
        dispatch({ type: "set-status", status: { type: "two-factor", email: parsed.data.email } });
        return;
      }
      dispatch({ type: "set-status", status: { type: "success", message: copy.status.sessionConfirmed } });
      push(callbackUrl as Route);
    } catch (error) {
      setError(getErrorMessage(error, copy.status.genericError));
    }
  };

  const submitForgot = async () => {
    const parsed = forgotSchema.safeParse({ email: values.email });
    if (!parsed.success) return setError(parsed.error.issues[0]?.message ?? copy.status.checkForm);
    const error = await requestPasswordReset(parsed.data.email, toBrowserUrl("/login"));
    if (error) return setError(error);
    dispatch({ type: "set-status", status: { type: "success", message: copy.status.resetSent } });
  };

  const submitReset = async () => {
    const parsed = resetSchema.safeParse({ password: values.password, confirmPassword: values.confirmPassword });
    if (!parsed.success) return setError(parsed.error.issues[0]?.message ?? copy.status.checkForm);
    if (!resetToken) return setError(copy.status.genericError);
    const error = await resetPassword(parsed.data.password, resetToken);
    if (error) return setError(error);
    dispatch({ type: "set-mode", mode: "signin" });
    dispatch({ type: "set-status", status: { type: "success", message: copy.status.resetDone } });
  };

  const submitSso = async () => {
    const parsed = ssoSchema.safeParse({ email: values.businessEmail, domain: values.orgDomain });
    if (!parsed.success) return setError(parsed.error.issues[0]?.message ?? copy.validation.domainOrEmail);
    dispatch({ type: "set-status", status: { type: "loading", message: copy.status.ssoOpening } });
    const error = await beginSsoSignIn({
      email: parsed.data.email || undefined,
      domain: parsed.data.domain || undefined,
      callbackURL: toBrowserUrl(callbackUrl),
    });
    if (error) setError(error);
  };

  const verifyTwoFactor = async (event: React.FormEvent) => {
    event.preventDefault();
    const code = values.twoFactorCode.replace(/\D/g, "");
    if (code.length < 6) return setError(copy.validation.codeRequired);
    dispatch({ type: "set-status", status: { type: "loading", message: copy.status.twoFactorVerifying } });
    try {
      const result = await authClient.twoFactor.verifyTotp({ code, trustDevice: true });
      if (result.error) return setError(getErrorMessage(result.error, copy.status.genericError));
      dispatch({ type: "set-status", status: { type: "success", message: copy.status.twoFactorConfirmed } });
      push(callbackUrl as Route);
    } catch (error) {
      setError(getErrorMessage(error, copy.status.genericError));
    }
  };

  const beginSocialSignIn = async (provider: "google" | "microsoft") => {
    dispatch({
      type: "set-status",
      status: { type: "loading", message: formatAuthText(copy.status.socialOpening, { provider: toTitleCase(provider) }) },
    });
    try {
      const result = await authClient.signIn.social({ provider, callbackURL: toBrowserUrl(callbackUrl) });
      if (result.error) setError(getErrorMessage(result.error, copy.status.genericError));
    } catch (error) {
      setError(getErrorMessage(error, copy.status.genericError));
    }
  };

  const beginPasskeySignIn = async () => {
    dispatch({ type: "set-status", status: { type: "loading", message: copy.status.passkeyOpening } });
    try {
      const result = await authClient.signIn.passkey();
      if (result?.error) return setError(getErrorMessage(result.error, copy.status.genericError));
      window.location.assign(callbackUrl);
    } catch (error) {
      setError(getErrorMessage(error, copy.status.genericError));
    }
  };

  // Conditional-mediation passkey autofill on the email field (best effort).
  useEffect(() => {
    if (!passkeySupported || mode !== "signin") return;
    let cancelled = false;
    void (async () => {
      if (!(await isConditionalMediationAvailable()) || cancelled) return;
      try {
        const client = authClient as unknown as {
          signIn: { passkey: (opts?: { autoFill?: boolean }) => Promise<{ error?: unknown } | undefined> };
        };
        await client.signIn.passkey({ autoFill: true });
      } catch {
        /* autofill is best effort */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [passkeySupported, mode]);

  function setError(message: string) {
    dispatch({ type: "set-status", status: { type: "error", message } });
  }

  const rememberCookieConsent = (choice: "accepted" | "rejected") => {
    document.cookie = `velion_cookie_consent=${choice}; path=/; max-age=31536000; SameSite=Lax`;
    dispatch({ type: "remember-cookie-consent", choice });
  };

  const busy = status.type === "loading";
  const cardScale = viewportHeight ? Math.min(1, Math.max(0.52, (viewportHeight - 18) / 1080)) : 1;

  return (
    <div
      className="auth-grain relative isolate z-40 flex h-[100dvh] min-h-[100dvh] items-center justify-center overflow-hidden px-3 py-0 opacity-100 transition-opacity duration-700 ease-out sm:px-4 md:px-5 lg:px-6 xl:px-10"
      style={{ "--primary": "#111111", "--primary-foreground": "#ffffff", "--ring": "#111111" } as React.CSSProperties}
    >
      <div
        className="relative z-[120] grid w-full max-w-[70.5rem] overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] shadow-[0_20px_50px_rgba(0,0,0,0.14)] md:grid-cols-[1.15fr_0.85fr] xl:max-w-[72rem]"
        style={{ transform: `scale(${cardScale})`, transformOrigin: "center center" }}
      >
        <AuthFormPane
          busy={busy}
          copy={copy}
          locale={locale}
          mode={mode}
          passkeySupported={passkeySupported}
          showPassword={showPassword}
          status={status}
          strength={strength}
          values={values}
          onModeChange={switchMode}
          onPasskeySignIn={beginPasskeySignIn}
          onSocialSignIn={beginSocialSignIn}
          onSubmit={submit}
          onTogglePassword={() => dispatch({ type: "toggle-password" })}
          onUpdateValue={updateValue}
          onVerifyTwoFactor={verifyTwoFactor}
        />
        <AuthVisualPane
          copy={copy}
          cookieConsent={cookieConsent}
          cookieSettingsOpen={cookieSettingsOpen}
          onAcceptCookies={() => rememberCookieConsent("accepted")}
          onRejectCookies={() => rememberCookieConsent("rejected")}
          onToggleCookieSettings={() => dispatch({ type: "open-cookie-settings" })}
        />
      </div>

      <AuthFooter copy={copy} onOpenCookieSettings={() => dispatch({ type: "open-cookie-settings" })} />
      <AuthBrandButton />
      <CookiePreferencesDialog
        copy={copy}
        open={cookieSettingsOpen}
        onClose={() => dispatch({ type: "set-cookie-settings-open", open: false })}
        onAccept={rememberCookieConsent}
      />
    </div>
  );
}

function AuthFooter({ copy, onOpenCookieSettings }: { copy: AuthCopy; onOpenCookieSettings: () => void }) {
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-3 hidden text-center md:block">
      <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-6 text-xs tracking-[0.02em] text-[#6A655F]">
        {copy.footer.map((item, index) => (
          <button
            key={item}
            type="button"
            onClick={index === copy.footer.length - 1 ? onOpenCookieSettings : undefined}
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

interface FormPaneProps {
  busy: boolean;
  copy: AuthCopy;
  locale: AuthLocale;
  mode: AuthMode;
  passkeySupported: boolean;
  showPassword: boolean;
  status: AuthStatus;
  strength: ReturnType<typeof calculatePasswordStrength>;
  values: AuthValues;
  onModeChange: (mode: AuthMode) => void;
  onPasskeySignIn: () => void;
  onSocialSignIn: (provider: "google" | "microsoft") => void;
  onSubmit: (event: React.FormEvent) => void;
  onTogglePassword: () => void;
  onUpdateValue: (field: keyof AuthValues, value: string) => void;
  onVerifyTwoFactor: (event: React.FormEvent) => void;
}

function AuthFormPane(props: FormPaneProps) {
  const { copy, mode, status, busy } = props;
  const contentRef = useRef<HTMLDivElement | null>(null);
  const [contentHeight, setContentHeight] = useState<number | undefined>(undefined);

  useEffect(() => {
    const node = contentRef.current;
    if (!node) return;
    const measure = () => {
      const next = node.scrollHeight;
      setContentHeight((current) => (current === next ? current : next));
    };
    const frame = window.requestAnimationFrame(measure);
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(measure) : null;
    ro?.observe(node);
    const mo = typeof MutationObserver !== "undefined" ? new MutationObserver(measure) : null;
    mo?.observe(node, { childList: true, subtree: true, characterData: true });
    return () => {
      window.cancelAnimationFrame(frame);
      ro?.disconnect();
      mo?.disconnect();
    };
  }, []);

  const animatedContentHeight = contentHeight
    ? Math.max(0, contentHeight + (mode === "signin" ? 6 : mode === "signup" ? -18 : 0))
    : undefined;

  const showTabs = mode === "signin" || mode === "signup";
  const title = copy.modes[mode].title;
  const submitLabel =
    mode === "signup"
      ? copy.buttons.signup
      : mode === "forgot"
        ? copy.buttons.sendReset
        : mode === "reset"
          ? copy.buttons.reset
          : mode === "sso"
            ? copy.buttons.sso
            : copy.buttons.signin;

  return (
    <section className="flex items-center justify-center rounded-[24px] bg-white px-5 py-6 sm:p-7 md:rounded-l-[24px] md:rounded-r-none md:p-8 lg:px-10 lg:py-9 xl:px-16 xl:py-10">
      <div className="w-full max-w-[21rem] sm:max-w-[22rem] lg:max-w-[22.75rem] xl:max-w-[25rem]">
        <div className="mb-4 flex items-center justify-between xl:mb-6">
          <Link href="/" className="text-xs font-bold tracking-tight text-[#111111] transition-colors hover:text-[#FF2E63]">
            {copy.back}
          </Link>
          <AuthLanguageSwitcher />
        </div>

        {showTabs ? <AuthTabs copy={copy} mode={mode} disabled={busy} onChange={props.onModeChange} /> : null}

        <div className="mb-4 text-left xl:mb-5">
          <h1 className="font-inter text-[clamp(40px,5.1vw,72px)] font-[450] leading-none tracking-normal text-[#1C1C1C]">
            {toTitleCase(title)}
          </h1>
          <p className="mt-3 text-[15px] leading-[1.6] text-[#66615B] xl:mt-4">{copy.modes[mode].description}</p>
        </div>

        <div className="relative overflow-hidden transition-[height] duration-[520ms] ease-in-out" style={{ height: animatedContentHeight ? `${animatedContentHeight}px` : undefined }}>
          <div ref={contentRef} className="transform-gpu">
            {status.type === "two-factor" ? (
              <TwoFactorForm copy={copy} code={props.values.twoFactorCode} email={status.email} onCodeChange={(v) => props.onUpdateValue("twoFactorCode", v)} onSubmit={props.onVerifyTwoFactor} />
            ) : status.type === "verify-email" ? (
              <VerifyEmailNotice copy={copy} email={status.email} onBack={() => props.onModeChange("signin")} />
            ) : (
              <AuthMainForm {...props} submitLabel={submitLabel} />
            )}

            <AuthStatusLine status={status} />

            {(mode === "signin" || mode === "signup") && status.type !== "verify-email" ? (
              <>
                <SocialProviderRow copy={copy} busy={busy} onMicrosoft={() => props.onSocialSignIn("microsoft")} onGoogle={() => props.onSocialSignIn("google")} onSso={() => props.onModeChange("sso")} />
                {props.passkeySupported ? (
                  <button
                    type="button"
                    disabled={busy}
                    onClick={props.onPasskeySignIn}
                    className="mt-4 inline-flex h-10 w-full items-center justify-center gap-2 rounded-lg border border-[#D6D2CB] bg-white text-sm font-medium text-[#1C1C1C] transition-colors hover:border-[#A09890] hover:bg-[#FAFAFA] disabled:opacity-50"
                  >
                    <KeyRound className="size-4" />
                    {copy.buttons.passkey}
                  </button>
                ) : null}
                <AuthTermsNotice copy={copy} mode={mode} />
              </>
            ) : null}
          </div>
        </div>

        <div className="mt-7 hidden text-left sm:block xl:mt-8">
          <p className="text-xs tracking-[0.02em] text-[#6A655F]">
            {copy.support.needHelp}{" "}
            <a href={`mailto:${copy.support.email}`} className="text-[#3E3A35] transition-colors hover:text-[#1C1C1C]">
              {copy.support.contact}
            </a>
          </p>
        </div>
      </div>
    </section>
  );
}

function AuthMainForm(props: FormPaneProps & { submitLabel: string }) {
  const { copy, mode, values, showPassword, busy, status, strength, submitLabel } = props;
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const fieldErrors = useMemo(() => computeFieldErrors(mode, values, copy), [mode, values, copy]);

  const fieldState = (field: string) => {
    if (!touched[field] || !values[field as keyof AuthValues]) return undefined;
    return fieldErrors[field] ? { valid: false } : { valid: true };
  };
  const markTouched = (field: string) => setTouched((prev) => ({ ...prev, [field]: true }));

  return (
    <form onSubmit={props.onSubmit} className="space-y-4">
      {mode === "signup" ? (
        <AuthField id="name" label={`${copy.fields.name} *`} icon={User} value={values.name} autoComplete="name" placeholder={copy.placeholders.name} validation={fieldState("name")} error={touched.name ? fieldErrors.name : undefined} onChange={(v) => props.onUpdateValue("name", v)} onBlur={() => markTouched("name")} />
      ) : null}

      {mode === "sso" ? (
        <>
          <AuthField id="businessEmail" label={`${copy.fields.businessEmail} *`} icon={Mail} type="email" value={values.businessEmail} autoComplete="email" placeholder={copy.placeholders.businessEmail} onChange={(v) => props.onUpdateValue("businessEmail", v)} />
          <AuthField id="orgDomain" label={`${copy.fields.orgDomain} ${copy.placeholders.optional}`} icon={Building2} value={values.orgDomain} placeholder={copy.placeholders.orgDomain} onChange={(v) => props.onUpdateValue("orgDomain", v)} />
        </>
      ) : null}

      {mode === "signin" || mode === "signup" || mode === "forgot" ? (
        <AuthField id="email" label={`${copy.fields.email} *`} icon={Mail} type="email" value={values.email} autoComplete="email webauthn" placeholder={copy.placeholders.email} validation={fieldState("email")} error={touched.email ? fieldErrors.email : undefined} onChange={(v) => props.onUpdateValue("email", v)} onBlur={() => markTouched("email")} />
      ) : null}

      {mode === "signin" || mode === "signup" || mode === "reset" ? (
        <AuthField
          id="password"
          label={`${mode === "reset" ? copy.fields.newPassword : copy.fields.password} *`}
          icon={Lock}
          type={showPassword ? "text" : "password"}
          value={values.password}
          autoComplete={mode === "signin" ? "current-password" : "new-password"}
          placeholder={copy.placeholders.password}
          validation={mode !== "signin" ? fieldState("password") : undefined}
          error={mode !== "signin" && touched.password ? fieldErrors.password : undefined}
          onChange={(v) => props.onUpdateValue("password", v)}
          onBlur={() => markTouched("password")}
          rightSlot={
            <button type="button" onClick={props.onTogglePassword} className="rounded p-1 text-[#8A8D96] transition-colors hover:text-[#1C1C1C]" aria-label={showPassword ? copy.fields.password : copy.fields.password}>
              {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </button>
          }
        />
      ) : null}

      {mode === "signup" || mode === "reset" ? (
        <>
          {mode === "signup" ? <PasswordStrengthBar copy={copy} score={strength.score} missing={strength.missing} /> : null}
          <AuthField id="confirmPassword" label={`${copy.fields.confirmPassword} *`} icon={Lock} type="password" value={values.confirmPassword} autoComplete="new-password" placeholder={copy.placeholders.password} validation={fieldState("confirmPassword")} error={touched.confirmPassword ? fieldErrors.confirmPassword : undefined} onChange={(v) => props.onUpdateValue("confirmPassword", v)} onBlur={() => markTouched("confirmPassword")} />
        </>
      ) : null}

      <button type="submit" disabled={busy} className="inline-flex h-11 w-full items-center justify-center rounded-[10px] bg-[#111111] px-5 text-[15px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50">
        {status.type === "loading" ? status.message : submitLabel}
      </button>

      {mode === "signin" ? (
        <div className="flex items-center justify-between pt-0.5">
          <button type="button" onClick={() => props.onModeChange("forgot")} className="text-xs font-medium text-[#3E3A35] underline-offset-2 transition-colors hover:text-[#111111] hover:underline">
            {copy.links.forgot}
          </button>
          <button type="button" onClick={() => props.onModeChange("sso")} className="text-xs font-medium text-[#3E3A35] underline-offset-2 transition-colors hover:text-[#111111] hover:underline">
            {copy.links.sso}
          </button>
        </div>
      ) : null}

      {mode === "forgot" || mode === "sso" || mode === "reset" ? (
        <button type="button" onClick={() => props.onModeChange("signin")} className="text-xs font-medium text-[#6A655F] underline-offset-2 transition-colors hover:text-[#111111] hover:underline">
          {copy.buttons.backToSignin}
        </button>
      ) : null}
    </form>
  );
}

function computeFieldErrors(mode: AuthMode, values: AuthValues, copy: AuthCopy): Record<string, string | null> {
  const errors: Record<string, string | null> = {};
  const emailValid = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(values.email);
  if ((mode === "signin" || mode === "signup" || mode === "forgot") && values.email) {
    errors.email = emailValid ? null : copy.validation.emailInvalid;
  }
  if (mode === "signup") {
    if (values.name) errors.name = values.name.trim().length >= 2 ? null : copy.validation.nameRequired;
  }
  if ((mode === "signup" || mode === "reset") && values.password) {
    errors.password = values.password.length >= 12 ? null : copy.validation.passwordMin;
  }
  if ((mode === "signup" || mode === "reset") && values.confirmPassword) {
    errors.confirmPassword = values.confirmPassword === values.password ? null : copy.validation.passwordMatch;
  }
  return errors;
}

function AuthLanguageSwitcher() {
  const { currentLocale, setLocale } = useLocale();
  const [open, setOpen] = useState(false);
  const languages: { code: Locale; name: string; flag: string }[] = [
    { code: "nb", name: "Norsk", flag: "🇳🇴" },
    { code: "en", name: "English", flag: "🇬🇧" },
  ];
  return (
    <div className="relative flex items-center">
      <button
        type="button"
        aria-label="Select language"
        aria-expanded={open}
        aria-haspopup="true"
        onClick={() => setOpen((v) => !v)}
        className="inline-flex items-center gap-2 rounded-lg bg-background/80 p-2 text-sm font-medium text-muted-foreground shadow-sm backdrop-blur-sm transition-colors hover:text-foreground hover:shadow-md focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
      >
        <Globe className="size-4" />
        <span className="text-xs uppercase tracking-[0.08em]">{currentLocale}</span>
        <ChevronDown className={cn("size-4 transition-transform", open ? "rotate-180" : "rotate-0")} />
      </button>
      <span className="ml-2 inline-block size-1.5 rounded-full bg-[#FF2E63]/70" aria-hidden="true" />
      {open ? (
        <div className="absolute right-0 top-10 z-[300] min-w-[140px] rounded-lg border border-border bg-background py-2 shadow-lg">
          {languages.map((language) => {
            const selected = currentLocale === language.code;
            return (
              <button
                key={language.code}
                type="button"
                onClick={() => {
                  setLocale(language.code);
                  setOpen(false);
                }}
                className={cn("flex w-full items-center gap-3 px-4 py-3 text-base transition-colors hover:bg-muted", selected ? "text-foreground" : "text-muted-foreground")}
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

function AuthTabs({ copy, mode, disabled, onChange }: { copy: AuthCopy; mode: AuthMode; disabled: boolean; onChange: (mode: AuthMode) => void }) {
  const tabs = [
    { id: "signin" as const, label: copy.tabs.signin, icon: LogIn },
    { id: "signup" as const, label: copy.tabs.signup, icon: UserPlus },
  ];
  return (
    <div className="mb-6 mt-0.5 sm:mb-8" role="tablist" aria-label="Authentication">
      <div className="inline-flex rounded-full bg-[#F0EFED] p-1 shadow-[inset_0_1px_1px_rgba(0,0,0,0.04),0_6px_16px_rgba(0,0,0,0.06)]">
        {tabs.map((candidate) => {
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
                active ? "bg-white text-[#1C1C1E] shadow ring-1 ring-[#D6D2CB]" : "text-[#777169] hover:bg-white/70 hover:text-[#1C1C1E]",
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
  onBlur,
  type = "text",
  autoComplete,
  inputMode,
  placeholder,
  rightSlot,
  validation,
  error,
}: {
  id: string;
  label: string;
  icon: LucideIcon;
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  type?: string;
  autoComplete?: string;
  inputMode?: React.HTMLAttributes<HTMLInputElement>["inputMode"];
  placeholder?: string;
  rightSlot?: React.ReactNode;
  validation?: { valid: boolean };
  error?: string | null;
}) {
  const borderClass = error
    ? "border-[#B42318] focus:border-[#B42318] focus:ring-[#B42318]/10"
    : validation?.valid
      ? "border-[#107A55] focus:border-[#107A55] focus:ring-[#107A55]/10"
      : "border-[#D6D2CB] focus:border-[#111111] focus:ring-[#111111]/10";
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
          aria-invalid={Boolean(error)}
          required={label.includes("*")}
          value={value}
          autoComplete={autoComplete}
          inputMode={inputMode}
          placeholder={placeholder}
          onChange={(event) => onChange(event.target.value)}
          onBlur={onBlur}
          className={cn("h-10 w-full rounded-lg border bg-white py-2 pl-10 pr-10 text-sm text-[#1C1C1C] placeholder:text-[#A09890] transition-colors focus:outline-none focus:ring-2", borderClass)}
        />
        {validation?.valid && !rightSlot ? <Check className="absolute right-3 size-4 text-[#107A55]" /> : null}
        {rightSlot ? <span className="absolute right-2">{rightSlot}</span> : null}
      </span>
      {error ? (
        <span role="alert" className="mt-1 block text-xs text-[#B42318]">
          {error}
        </span>
      ) : null}
    </label>
  );
}

function PasswordStrengthBar({ copy, score, missing }: { copy: AuthCopy; score: number; missing: PasswordStrengthKey[] }) {
  const filled = Math.min(5, Math.max(0, score));
  return (
    <div className="space-y-2">
      <div className="flex gap-1">
        {Array.from({ length: 5 }).map((_, index) => (
          <span key={index} className={cn("h-1 flex-1 rounded-full", index < filled ? "bg-[#111111]" : "bg-[#E5DFD3]")} />
        ))}
      </div>
      {missing.length > 0 ? (
        <p className="text-xs text-[#6A655F]">
          {copy.strength.add} {missing.slice(0, 3).map((key) => copy.strength[key]).join(", ")}.
        </p>
      ) : (
        <p className="text-xs text-[#107A55]">{copy.strength.strong}</p>
      )}
    </div>
  );
}

function TwoFactorForm({ copy, code, email, onCodeChange, onSubmit }: { copy: AuthCopy; code: string; email: string; onCodeChange: (value: string) => void; onSubmit: (event: React.FormEvent) => void }) {
  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="rounded-xl border border-[#E5DFD3] bg-[#F7F4ED] p-4">
        <div className="flex items-start gap-3">
          <span className="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white">
            <KeyRound className="size-4" />
          </span>
          <div>
            <h2 className="text-sm font-semibold text-[#1C1C1C]">{copy.twoFactor.title}</h2>
            <p className="mt-1 text-xs leading-5 text-[#6A655F]">
              {copy.twoFactor.description} {copy.twoFactor.sentTo} {email}.
            </p>
          </div>
        </div>
      </div>
      <AuthField id="twoFactorCode" label={copy.twoFactor.codeLabel} icon={KeyRound} value={code} inputMode="numeric" placeholder={copy.placeholders.code} onChange={(v) => onCodeChange(v.replace(/\D/g, "").slice(0, 6))} />
      <button type="submit" className="inline-flex w-full items-center justify-center rounded-md bg-[#111111] px-5 py-3 text-[11px] font-semibold uppercase tracking-[0.22em] text-white transition-opacity hover:opacity-80">
        {copy.twoFactor.continue}
      </button>
      <button type="button" className="text-[11px] uppercase tracking-[0.18em] text-[#A09890] hover:text-[#111111]">
        {copy.twoFactor.backupLabel}
      </button>
    </form>
  );
}

function VerifyEmailNotice({ copy, email, onBack }: { copy: AuthCopy; email: string; onBack: () => void }) {
  return (
    <div className="space-y-4">
      <div className="rounded-xl border border-[#E5DFD3] bg-[#F7F4ED] p-4">
        <div className="flex items-start gap-3">
          <span className="grid size-9 shrink-0 place-items-center rounded-full bg-[#111111] text-white">
            <Mail className="size-4" />
          </span>
          <div>
            <h2 className="text-sm font-semibold text-[#1C1C1C]">{copy.verifyEmail.title}</h2>
            <p className="mt-1 text-xs leading-5 text-[#6A655F]">{formatAuthText(copy.verifyEmail.description, { email })}</p>
          </div>
        </div>
      </div>
      <button type="button" onClick={onBack} className="text-xs font-medium text-[#6A655F] underline-offset-2 transition-colors hover:text-[#111111] hover:underline">
        {copy.buttons.backToSignin}
      </button>
    </div>
  );
}

function AuthStatusLine({ status }: { status: AuthStatus }) {
  if (status.type === "idle" || status.type === "two-factor" || status.type === "verify-email") return null;
  const tone = status.type === "error" ? "text-[#B42318]" : status.type === "success" ? "text-[#107A55]" : "text-[#6A655F]";
  return (
    <p role={status.type === "error" ? "alert" : undefined} className={cn("mt-3 text-xs leading-5", tone)}>
      {status.message}
    </p>
  );
}

function AuthTermsNotice({ copy, mode }: { copy: AuthCopy; mode: AuthMode }) {
  return (
    <div className="mt-3 text-left">
      <p className="text-xs leading-[1.65] tracking-[0.02em] text-[#6A655F]">
        {mode === "signin" ? (
          <>
            {copy.terms.prefixSignin}{" "}
            <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C]">{copy.terms.user}</button>{" "}
            {copy.terms.and}{" "}
            <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C]">{copy.terms.privacy}</button>.
          </>
        ) : (
          <>
            {copy.terms.signupNotice}{" "}
            <button type="button" className="text-[#3E3A35] underline hover:text-[#1C1C1C]">{copy.terms.deleteCookie}</button>.
          </>
        )}
      </p>
    </div>
  );
}

function SocialProviderRow({ copy, busy, onMicrosoft, onGoogle, onSso }: { copy: AuthCopy; busy: boolean; onMicrosoft: () => void; onGoogle: () => void; onSso: () => void }) {
  return (
    <div className="mt-5">
      <div className="relative flex items-center justify-center">
        <div className="h-px flex-1 bg-[#E5E1DA]" />
        <span className="mx-3 bg-white px-2 text-[13px] text-[#7B756E]">{copy.divider}</span>
        <div className="h-px flex-1 bg-[#E5E1DA]" />
      </div>
      <div className="mt-4 flex items-center justify-center gap-14">
        <SocialCircleButton label={copy.social.microsoft} disabled={busy} onClick={onMicrosoft}>
          <MicrosoftGlyph />
        </SocialCircleButton>
        <SocialCircleButton label={copy.social.google} disabled={busy} onClick={onGoogle}>
          <GoogleGlyph />
        </SocialCircleButton>
        <SocialCircleButton label={copy.social.sso} disabled={busy} onClick={onSso}>
          <CircleDot className="size-5 text-[#A6ABB4]" />
        </SocialCircleButton>
      </div>
    </div>
  );
}

function SocialCircleButton({ label, disabled, onClick, children }: { label: string; disabled: boolean; onClick: () => void; children: React.ReactNode }) {
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
  copy,
  cookieConsent,
  cookieSettingsOpen,
  onAcceptCookies,
  onRejectCookies,
  onToggleCookieSettings,
}: {
  copy: AuthCopy;
  cookieConsent: "accepted" | "rejected" | null;
  cookieSettingsOpen: boolean;
  onAcceptCookies: () => void;
  onRejectCookies: () => void;
  onToggleCookieSettings: () => void;
}) {
  return (
    <aside className="relative hidden min-h-[560px] overflow-hidden rounded-r-[24px] md:block lg:min-h-[600px] xl:min-h-[640px]">
      <div className="absolute inset-0 bg-cover bg-center" style={{ backgroundImage: "url('/imagens/curved-interior-sculpture.png')" }} />
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
      <CookieConsentCard copy={copy} consent={cookieConsent} settingsOpen={cookieSettingsOpen} onAccept={onAcceptCookies} onReject={onRejectCookies} onToggleSettings={onToggleCookieSettings} />
      <style>{`
        .scanner-dot { animation: scannerMove 5.5s ease-in-out infinite alternate; }
        @keyframes scannerMove { 0% { top: 0; } 100% { top: calc(100% - 20px); } }
        @media (prefers-reduced-motion: reduce) { .scanner-dot { animation: none; } }
      `}</style>
    </aside>
  );
}

function CookieConsentCard({
  copy,
  consent,
  settingsOpen,
  onAccept,
  onReject,
  onToggleSettings,
}: {
  copy: AuthCopy;
  consent: "accepted" | "rejected" | null;
  settingsOpen: boolean;
  onAccept: () => void;
  onReject: () => void;
  onToggleSettings: () => void;
}) {
  if (consent && !settingsOpen) return null;
  return (
    <div className="velion-cookie-card absolute bottom-6 left-1/2 z-30 hidden w-[92%] -translate-x-1/2 lg:block">
      <div className="rounded-full bg-background px-3 py-2 shadow-lg">
        <div className="flex items-center gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-[#F2F3F5] text-[#6A6E78]">
            <CookieGlyph />
          </span>
          <p className="min-w-0 flex-1 text-sm leading-tight text-foreground/90">{copy.cookie.message}</p>
          <div className="flex shrink-0 items-center gap-2">
            <button type="button" aria-label={copy.cookie.settings} title={copy.cookie.settings} onClick={onToggleSettings} className="size-9 rounded-full bg-background ring-1 ring-border transition hover:bg-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-ring">
              <ListCheck className="mx-auto size-[18px] text-foreground/70" />
            </button>
            <button type="button" onClick={onReject} className="h-9 rounded-bl-[30px] rounded-br-[10px] rounded-tl-[30px] rounded-tr-[10px] bg-background px-4 text-foreground ring-1 ring-border transition hover:bg-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-ring">
              {copy.cookie.reject}
            </button>
            <button type="button" onClick={onAccept} className="h-9 rounded-bl-[10px] rounded-br-[30px] rounded-tl-[10px] rounded-tr-[30px] bg-primary px-5 font-medium text-primary-foreground transition hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring">
              {copy.cookie.accept}
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
  copy,
  open,
  onClose,
  onAccept,
}: {
  copy: AuthCopy;
  open: boolean;
  onClose: () => void;
  onAccept: (choice: "accepted" | "rejected") => void;
}) {
  const closeDialog = useEffectEvent(() => {
    onClose();
  });

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeDialog();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;
  const categories = [
    { key: "necessary" as const, label: copy.cookie.categories.necessary, note: copy.cookie.necessaryNote, on: true },
    { key: "preferences" as const, label: copy.cookie.categories.preferences, note: copy.cookie.adjustableNote, on: false },
    { key: "analytics" as const, label: copy.cookie.categories.analytics, note: copy.cookie.adjustableNote, on: false },
  ];
  return (
    <dialog open aria-labelledby="cookie-prefs-title" aria-modal="true" className="fixed inset-0 z-[300] m-0 flex h-auto max-h-none w-auto max-w-none items-center justify-center border-0 bg-transparent p-0 text-inherit">
      <button type="button" aria-label={copy.cookie.close} className="absolute inset-0 bg-background/60 backdrop-blur-sm" onClick={onClose} />
      <div className="relative max-h-[90svh] w-[92vw] max-w-xl overflow-y-auto rounded-2xl bg-card text-card-foreground shadow-2xl">
        <div className="grid grid-cols-3 items-center p-4">
          <button type="button" onClick={onClose} className="flex size-8 items-center justify-center rounded-full bg-muted text-foreground/80 transition hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={copy.cookie.close}>
            <X className="size-4" />
          </button>
          <h2 id="cookie-prefs-title" className="justify-self-center text-sm font-medium">{copy.cookie.settings}</h2>
          <span className="justify-self-end" aria-hidden="true" />
        </div>
        <div className="border-b border-border p-4">
          <p className="text-sm text-muted-foreground">{copy.cookie.dialogIntro}</p>
          <button type="button" onClick={() => onAccept("accepted")} className="mt-4 h-10 w-full rounded-full bg-primary font-medium text-primary-foreground transition hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring">
            {copy.cookie.allowAll}
          </button>
        </div>
        <div className="space-y-3 p-4">
          {categories.map((category) => (
            <div key={category.key} className="flex items-center justify-between rounded-xl border border-border bg-background p-3">
              <div>
                <p className="text-sm font-medium">{category.label}</p>
                <p className="mt-1 text-xs text-muted-foreground">{category.note}</p>
              </div>
              <span className={cn("relative h-6 w-11 rounded-full ring-1 ring-border after:absolute after:left-0.5 after:top-0.5 after:size-5 after:rounded-full after:bg-background after:transition-transform", category.on ? "bg-primary after:translate-x-5" : "bg-muted")} aria-hidden="true" />
            </div>
          ))}
        </div>
        <div className="sticky bottom-0 flex justify-end gap-2 border-t border-border bg-card p-4">
          <button type="button" onClick={() => onAccept("rejected")} className="rounded-bl-[30px] rounded-br-[10px] rounded-tl-[30px] rounded-tr-[10px] bg-muted px-4 py-2 text-foreground transition hover:opacity-90">
            {copy.cookie.rejectAll}
          </button>
          <button type="button" onClick={() => onAccept("accepted")} className="rounded-bl-[10px] rounded-br-[30px] rounded-tl-[10px] rounded-tr-[30px] bg-primary px-4 py-2 text-primary-foreground transition hover:bg-primary/90">
            {copy.cookie.saveChoice}
          </button>
        </div>
      </div>
    </dialog>
  );
}
