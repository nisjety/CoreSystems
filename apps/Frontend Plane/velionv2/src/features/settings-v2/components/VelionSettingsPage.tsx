import { Check, ChevronDown } from "lucide-react";
import {
  VerevonButton,
  VerevonInput,
  VerevonSelect,
  VerevonSwitch,
  VerevonTextarea,
} from "@/components/ui/verevon-ui";
import { cn } from "@/lib/utils";
import { PasskeySecuritySection } from "@/features/settings-v2/components/PasskeySecuritySection";

const connectedAccounts = [
  { provider: "Google", detail: "author@verevon.ai", status: "Connected" },
  { provider: "Microsoft", detail: "Not connected", status: "Connect" },
  { provider: "GitHub", detail: "@author", status: "Connected" },
  { provider: "Slack", detail: "Triodelab workspace", status: "Connected" },
];

const accountSignals = [
  { label: "Profile", value: "82% complete" },
  { label: "Email", value: "Verified" },
  { label: "Scope", value: "Personal account" },
];

const supportPreferences = [
  {
    title: "Sound notifications",
    description: "Play a short tone for active helpdesk conversations.",
    enabled: true,
  },
  {
    title: "AI macro suggestions",
    description: "Show suggested macros when replying to tickets.",
    enabled: true,
  },
  {
    title: "Auto-open macro search",
    description: "Start ticket replies with the macro search view open.",
    enabled: false,
  },
  {
    title: "Forward calls when offline",
    description: "Send routed calls to your external phone number when unavailable.",
    enabled: false,
  },
];

export function VerevonSettingsPage() {
  return (
    <div
      data-account-settings-scroll
      className="relative h-full overflow-y-auto bg-[#F2F2F1] text-[#111111] [scrollbar-gutter:stable] dark:bg-[#111214] dark:text-[#F7F8F8]"
    >
      <div
        data-testid="settings-top-scroll-fade"
        aria-hidden="true"
        className="pointer-events-none sticky top-0 z-20 h-14 bg-gradient-to-b from-[#F2F2F1] via-[#F2F2F1]/88 to-transparent backdrop-blur-[2px] dark:from-[#111214] dark:via-[#111214]/88"
      />

      <div
        data-testid="settings-profile-content"
        className="-mt-8 mx-auto w-full max-w-[900px] px-5 pb-28 pt-7 lg:px-8"
      >
        <main className="min-w-0">
          <SettingsHero />
          <ProfileSection />
          <ContactSection />
          <PreferencesSection />
          <AvailabilitySection />
          <ConnectedAccountsSection />
          <PasskeySecuritySection />
          <PrivacySection />
          <SettingsActions />
        </main>
      </div>

      <div
        data-testid="settings-bottom-scroll-fade"
        aria-hidden="true"
        className="pointer-events-none sticky bottom-0 z-20 h-20 bg-gradient-to-t from-[#F2F2F1] via-[#F2F2F1]/82 to-transparent dark:from-[#111214] dark:via-[#111214]/82"
      />
    </div>
  );
}

function SettingsHero() {
  return (
    <div className="mb-12">
      <p className="mb-4 text-[11px] font-semibold uppercase tracking-[0.22em] text-[#9A9CA3]">Account</p>
      <h1 className="text-[36px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white sm:text-[40px]">
        Profile settings
      </h1>
      <dl className="mt-7 grid gap-3 border-y border-[#E7E7E8] py-4 sm:grid-cols-3 dark:border-white/10">
        {accountSignals.map((signal) => (
          <div key={signal.label} className="min-w-0">
            <dt className="text-[11px] font-medium uppercase tracking-[0.16em] text-[#9A9DA5]">
              {signal.label}
            </dt>
            <dd className="mt-1 truncate text-[13px] font-medium text-[#24262B] dark:text-[#ECEEF2]">
              {signal.value}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

function ProfileSection() {
  return (
    <section id="profile" className="scroll-mt-24">
      <SectionHeader
        title="Profile"
        description="Control how teammates and customers see you across Verevon."
      />

      <div className="mb-7 flex flex-col gap-5 sm:flex-row sm:items-center">
        <button
          type="button"
          aria-label="Update avatar"
          className="grid size-20 shrink-0 place-items-center rounded-full border border-white/80 bg-[radial-gradient(circle_at_35%_30%,#FFE7B0,transparent_34%),radial-gradient(circle_at_66%_28%,#7CC4FF,transparent_28%),linear-gradient(145deg,#F8D9C1,#D9DEE8)] text-[22px] font-semibold text-[#1A1A1A] shadow-[inset_0_1px_0_rgba(255,255,255,0.75),0_12px_28px_rgba(17,17,17,0.08)]"
        >
          AN
        </button>
        <div className="min-w-0">
          <p className="text-[13px] leading-5 text-[#676B74] dark:text-[#A9ADB6]">
            Upload a square JPG or PNG profile image. This is visible to teammates and customer-facing chat handoffs.
          </p>
          <div className="mt-4 flex flex-wrap gap-2">
            <VerevonButton variant="primary" size="sm" className="px-4">
              Upload photo
            </VerevonButton>
            <VerevonButton size="sm" className="px-4">
              Remove
            </VerevonButton>
          </div>
        </div>
      </div>

      <div className="grid gap-6 sm:grid-cols-2">
        <SettingsField id="display-name" label="Display name" defaultValue="Author Name" />
        <SettingsField
          id="username"
          label="Username"
          defaultValue="author"
          prefix="@"
          helpText="Available. Used in teammate mentions and profile links."
        />
        <SettingsField id="job-title" label="Job title" defaultValue="Customer support lead" />
        <SettingsField id="pronouns" label="Pronouns" placeholder="Optional" />
        <SettingsSelect
          id="customer-name-display"
          label="Name display"
          defaultValue="full-name"
          options={[
            { value: "full-name", label: "Show full name" },
            { value: "username", label: "Show username" },
          ]}
        />
        <SettingsSelect
          id="customer-facing-name"
          label="Customer-facing name"
          defaultValue="display-name"
          options={[
            { value: "display-name", label: "Author Name" },
            { value: "team-alias", label: "Verevon Support" },
            { value: "hidden", label: "Hide personal name" },
          ]}
        />
        <div className="sm:col-span-2">
          <SettingsTextarea
            id="bio"
            label="Bio"
            defaultValue="I help customers get clear answers quickly, and keep the knowledge base aligned with real support conversations."
          />
        </div>
        <div className="sm:col-span-2">
          <SettingsTextarea
            id="support-signature"
            label="Support signature"
            defaultValue={"Best,\nAuthor"}
          />
        </div>
      </div>
    </section>
  );
}

function ContactSection() {
  return (
    <section id="contact" className="mt-16 scroll-mt-24">
      <SectionHeader
        title="Contact"
        description="Keep sign-in and teammate contact details current."
      />
      <div className="grid gap-6 sm:grid-cols-2">
        <SettingsField
          id="email"
          label="Primary email"
          type="email"
          defaultValue="author@verevon.ai"
          helpText="Verified. Used for sign-in and account recovery."
        />
        <SettingsField id="secondary-email" label="Backup email" type="email" placeholder="backup@example.com" />
        <SettingsField id="phone" label="Phone number" type="tel" defaultValue="+47 400 00 000" />
        <SettingsField id="forwarding-number" label="Call forwarding number" type="tel" placeholder="+47 900 00 000" />
      </div>
    </section>
  );
}

function PreferencesSection() {
  return (
    <section id="preferences" className="mt-16 scroll-mt-24">
      <SectionHeader
        title="Preferences"
        description="Personalize how Verevon formats dates, language, appearance, and teammate names."
      />
      <div className="grid gap-6 sm:grid-cols-2">
        <SettingsSelect
          id="language"
          label="Language"
          defaultValue="en"
          options={[
            { value: "en", label: "English" },
            { value: "nb", label: "Norwegian Bokmal" },
            { value: "fr", label: "French" },
            { value: "de", label: "German" },
          ]}
        />
        <SettingsSelect
          id="timezone"
          label="Time zone"
          defaultValue="europe-oslo"
          options={[
            { value: "europe-oslo", label: "Europe/Oslo" },
            { value: "utc", label: "UTC" },
            { value: "america-new-york", label: "America/New York" },
            { value: "europe-london", label: "Europe/London" },
          ]}
        />
        <SettingsSelect
          id="date-format"
          label="Date format"
          defaultValue="dd-mm-yyyy"
          options={[
            { value: "dd-mm-yyyy", label: "DD/MM/YYYY" },
            { value: "mm-dd-yyyy", label: "MM/DD/YYYY" },
            { value: "yyyy-mm-dd", label: "YYYY-MM-DD" },
          ]}
        />
        <SettingsSelect
          id="time-format"
          label="Time format"
          defaultValue="24-hour"
          options={[
            { value: "24-hour", label: "24-hour" },
            { value: "12-hour", label: "12-hour AM/PM" },
          ]}
        />
        <SettingsSelect
          id="theme"
          label="Theme"
          defaultValue="system"
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
        <SettingsSelect
          id="email-digest"
          label="Email digest"
          defaultValue="daily"
          options={[
            { value: "realtime", label: "Realtime" },
            { value: "daily", label: "Daily summary" },
            { value: "weekly", label: "Weekly summary" },
            { value: "off", label: "Off" },
          ]}
        />
        <SettingsSelect
          id="first-day-of-week"
          label="First day of week"
          defaultValue="monday"
          options={[
            { value: "monday", label: "Monday" },
            { value: "sunday", label: "Sunday" },
          ]}
        />
      </div>
    </section>
  );
}

function AvailabilitySection() {
  return (
    <section id="availability" className="mt-16 scroll-mt-24">
      <SectionHeader
        title="Availability"
        description="Tune personal helpdesk behavior for conversations assigned to you."
      />
      <div className="grid gap-6 sm:grid-cols-2">
        <SettingsSelect
          id="availability-status"
          label="Availability status"
          defaultValue="available"
          options={[
            { value: "available", label: "Available" },
            { value: "busy", label: "Busy" },
            { value: "away", label: "Away" },
            { value: "offline", label: "Offline" },
          ]}
        />
        <SettingsSelect
          id="default-translation-language"
          label="Default translation language"
          defaultValue="english"
          options={[
            { value: "english", label: "English" },
            { value: "norwegian", label: "Norwegian" },
            { value: "french", label: "French" },
            { value: "spanish", label: "Spanish" },
          ]}
        />
        <SettingsField id="known-languages" label="Languages you know" defaultValue="English, Norwegian" />
        <SettingsField id="status-message" label="Status message" defaultValue="Available for priority handoffs" />
        <SettingsSelect
          id="reply-style"
          label="Reply style"
          defaultValue="balanced"
          options={[
            { value: "concise", label: "Concise" },
            { value: "balanced", label: "Balanced" },
            { value: "detailed", label: "Detailed" },
          ]}
        />
        <SettingsSelect
          id="default-inbox"
          label="Default inbox"
          defaultValue="assigned"
          options={[
            { value: "assigned", label: "Assigned to me" },
            { value: "unassigned", label: "Unassigned" },
            { value: "priority", label: "Priority" },
          ]}
        />
      </div>
      <div className="mt-7 divide-y divide-[#E8E8EA] border-y border-[#E8E8EA] dark:divide-white/10 dark:border-white/10">
        {supportPreferences.map((preference) => (
          <ToggleRow key={preference.title} {...preference} />
        ))}
      </div>
    </section>
  );
}

function ConnectedAccountsSection() {
  return (
    <section id="connected-accounts" className="mt-16 scroll-mt-24">
      <SectionHeader
        title="Connected accounts"
        description="Connect personal integrations used for sign-in, identity matching, and workflow attribution."
      />
      <div className="divide-y divide-[#E8E8EA] overflow-hidden rounded-[18px] border border-[#E1E2E4] bg-white/42 dark:divide-white/10 dark:border-white/10 dark:bg-white/5">
        {connectedAccounts.map((account) => (
          <div key={account.provider} className="flex items-center justify-between gap-4 px-5 py-4">
            <div className="min-w-0">
              <p className="text-[13px] font-medium text-[#111111] dark:text-white">{account.provider}</p>
              <p className="mt-1 truncate text-[12px] text-[#737780] dark:text-[#A9ADB6]">{account.detail}</p>
            </div>
            <VerevonButton size="sm" radius="sm" className="shrink-0 px-3 text-[12px]">
              {account.status}
            </VerevonButton>
          </div>
        ))}
      </div>
    </section>
  );
}

function PrivacySection() {
  return (
    <section id="privacy" className="mt-16 scroll-mt-24">
      <SectionHeader
        title="Privacy"
        description="Choose how discoverable your account is to other workspaces and how profile activity is stored."
      />
      <div className="divide-y divide-[#E8E8EA] border-y border-[#E8E8EA] dark:divide-white/10 dark:border-white/10">
        <ToggleRow
          title="Profile visibility"
          description="Allow people with your email address to see your name and avatar when inviting you."
          enabled
        />
        <ToggleRow
          title="Record profile activity"
          description="Include your profile views and contribution history in account activity."
          enabled={false}
        />
      </div>
    </section>
  );
}

function SettingsActions() {
  return (
    <div className="mt-10 flex flex-col gap-4 border-t border-[#E8E8EA] pt-6 sm:flex-row sm:items-center sm:justify-between dark:border-white/10">
      <p className="max-w-[360px] text-[12px] leading-5 text-[#747780] dark:text-[#A9ADB6]">
        Your personal profile, preferences, and security (passkeys) are managed here.
      </p>
      <div className="flex items-center justify-end gap-3">
        <VerevonButton className="px-5">
          Cancel
        </VerevonButton>
        <VerevonButton variant="primary" className="px-5">
          <Check className="size-4" strokeWidth={1.8} />
          Save profile
        </VerevonButton>
      </div>
    </div>
  );
}

function SectionHeader({ title, description }: { title: string; description: string }) {
  return (
    <div className="mb-7">
      <h2 className="text-[27px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white">{title}</h2>
      <p className="mt-2 max-w-[640px] text-[13px] leading-5 text-[#6A6E77] dark:text-[#A9ADB6]">{description}</p>
    </div>
  );
}

function SettingsField({
  id,
  label,
  defaultValue,
  placeholder,
  prefix,
  helpText,
  type = "text",
}: {
  id: string;
  label: string;
  defaultValue?: string;
  placeholder?: string;
  prefix?: string;
  helpText?: string;
  type?: "email" | "tel" | "text";
}) {
  const helpId = helpText ? `${id}-help` : undefined;

  return (
    <label htmlFor={id} className="block">
      <span className="verevon-settings-label">{label}</span>
      <div className="relative -mt-2">
        {prefix ? (
          <span className="pointer-events-none absolute left-5 top-1/2 -translate-y-1/2 text-[13px] font-medium text-[#858993]">
            {prefix}
          </span>
        ) : null}
        <VerevonInput
          id={id}
          type={type}
          defaultValue={defaultValue}
          placeholder={placeholder}
          aria-describedby={helpId}
          variant="settings"
          className={cn(
            prefix ? "pl-9" : "",
          )}
        />
      </div>
      {helpText ? (
        <span id={helpId} className="mt-2 block px-1 text-[12px] leading-5 text-[#737780] dark:text-[#A9ADB6]">
          {helpText}
        </span>
      ) : null}
    </label>
  );
}

function SettingsTextarea({
  id,
  label,
  defaultValue,
}: {
  id: string;
  label: string;
  defaultValue: string;
}) {
  return (
    <label htmlFor={id} className="block">
      <span className="verevon-settings-label">{label}</span>
      <VerevonTextarea
        id={id}
        defaultValue={defaultValue}
        rows={4}
        variant="settings"
        className="-mt-2"
      />
    </label>
  );
}

function SettingsSelect({
  id,
  label,
  defaultValue,
  options,
}: {
  id: string;
  label: string;
  defaultValue: string;
  options: Array<{ value: string; label: string }>;
}) {
  return (
    <label htmlFor={id} className="block">
      <span className="verevon-settings-label">{label}</span>
      <div className="relative -mt-2">
        <VerevonSelect
          id={id}
          defaultValue={defaultValue}
          variant="settings"
          className="appearance-none"
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </VerevonSelect>
        <ChevronDown
          aria-hidden="true"
          className="pointer-events-none absolute right-5 top-1/2 size-4 -translate-y-1/2 text-[#6F737C]"
          strokeWidth={1.7}
        />
      </div>
    </label>
  );
}

function ToggleRow({
  title,
  description,
  enabled,
}: {
  title: string;
  description: string;
  enabled: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-5 py-5">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-[#111111] dark:text-white">{title}</p>
        <p className="mt-1 text-[12px] leading-5 text-[#737780] dark:text-[#A9ADB6]">{description}</p>
      </div>
      <VerevonSwitch checked={enabled} label={title} />
    </div>
  );
}
