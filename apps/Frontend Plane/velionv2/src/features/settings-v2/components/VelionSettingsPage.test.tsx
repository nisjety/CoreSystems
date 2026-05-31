import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { VelionSettingsPage } from "@/features/settings-v2/components/VelionSettingsPage";

describe("VelionSettingsPage", () => {
  it("renders profile-only account settings in the widened layout", () => {
    render(<VelionSettingsPage />);

    expect(screen.getByRole("heading", { name: /profile settings/i, level: 1 })).toBeVisible();
    expect(screen.queryByRole("navigation", { name: /settings sections/i })).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-profile-content")).toHaveClass("max-w-[900px]");

    expect(screen.getByRole("textbox", { name: /display name/i })).toHaveValue("Author Name");
    expect(screen.getByRole("textbox", { name: /username/i })).toHaveValue("author");
    expect(screen.getByRole("textbox", { name: /job title/i })).toHaveValue("Customer support lead");
    expect(screen.getByRole("textbox", { name: /pronouns/i })).toBeVisible();
    expect(screen.getByRole("textbox", { name: /support signature/i })).toHaveValue("Best,\nAuthor");
    expect(screen.getByRole("textbox", { name: /primary email/i })).toHaveValue("author@velion.ai");
    expect(screen.getByRole("combobox", { name: /^language$/i })).toHaveValue("en");
    expect(screen.getByRole("combobox", { name: /^time zone$/i })).toHaveValue("europe-oslo");
    expect(screen.getByRole("combobox", { name: /^email digest$/i })).toHaveValue("daily");
    expect(screen.getByRole("combobox", { name: /^availability status$/i })).toHaveValue("available");
    expect(screen.getByRole("textbox", { name: /status message/i })).toHaveValue("Available for priority handoffs");
    expect(screen.getByRole("switch", { name: /sound notifications/i })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("Google")).toBeVisible();
    expect(screen.getByRole("switch", { name: /profile visibility/i })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText(/personal profile, preferences, and security/i)).toBeVisible();
    expect(screen.getByRole("button", { name: /save profile/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /^security$/i })).toBeVisible();
    expect(screen.queryByRole("heading", { name: /delete account/i })).not.toBeInTheDocument();
  });

  it("includes sticky scroll fade layers for the settings surface", () => {
    render(<VelionSettingsPage />);

    expect(screen.getByTestId("settings-top-scroll-fade")).toHaveClass("sticky");
    expect(screen.getByTestId("settings-bottom-scroll-fade")).toHaveClass("sticky");
  });
});
