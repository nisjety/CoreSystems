import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { VerevonWorkspaceSettingsPage } from "@/features/settings-v2/components/VerevonWorkspaceSettingsPage";
import { workspaceSettingsSections } from "@/features/settings-v2/lib/settings-sections";

describe("VerevonWorkspaceSettingsPage", () => {
  it("renders a single workspace settings page with mocked workspace controls, not planning notes", () => {
    render(<VerevonWorkspaceSettingsPage section="workspace" />);

    expect(screen.getByRole("heading", { name: /workspace settings/i, level: 1 })).toBeVisible();
    expect(screen.queryByRole("heading", { name: /must have/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /nice to have/i })).not.toBeInTheDocument();
    expect(screen.queryByText("Workspace name, URL, and primary domain")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /verified domains/i })).toBeVisible();
    expect(screen.getByText("support.aquatiq.no")).toBeVisible();
    expect(screen.getByRole("button", { name: /edit schedule/i })).toBeVisible();
    expect(screen.getByRole("textbox", { name: /workspace name/i })).toHaveValue("aquatiq-as");
    expect(screen.queryByRole("heading", { name: /members & roles/i })).not.toBeInTheDocument();
  });

  it("renders the members page as its own route-level section", () => {
    render(<VerevonWorkspaceSettingsPage section="members" />);

    expect(screen.getByRole("heading", { name: /members & roles/i, level: 1 })).toBeVisible();
    expect(screen.getByRole("textbox", { name: /invite by email/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /role templates/i })).toBeVisible();
    expect(screen.getByText("Full workspace, billing, and security")).toBeVisible();
    expect(screen.queryByRole("textbox", { name: /workspace name/i })).not.toBeInTheDocument();
  });

  it("renders security controls on the org security page", () => {
    render(<VerevonWorkspaceSettingsPage section="org-security" />);

    expect(screen.getByRole("heading", { name: /org security/i, level: 1 })).toBeVisible();
    expect(screen.getByText("Admins must use MFA for sensitive settings.")).toBeVisible();
    expect(screen.getByRole("switch", { name: /require mfa for admins/i })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("combobox", { name: /session duration/i })).toHaveValue("30-days");
    expect(screen.getByRole("heading", { name: /recent security events/i })).toBeVisible();
    expect(screen.getByText(/loading security events/i)).toBeVisible();
  });

  it("defines one route-level page for every settings sidebar tab", () => {
    workspaceSettingsSections.forEach((section) => {
      expect(section.id).toBeTruthy();
      expect(section.title).toBeTruthy();
      expect(section.description).toBeTruthy();
      expect(section.saveLabel).toMatch(/^Save /);
    });
  });

  it("renders billing and SSO feature stubs instead of visible guidance", () => {
    const { rerender } = render(<VerevonWorkspaceSettingsPage section="billing" />);

    expect(screen.getByRole("heading", { name: /billing/i, level: 1 })).toBeVisible();
    expect(screen.getByRole("heading", { name: /invoice history/i })).toBeVisible();
    expect(screen.getByText(/ingen fakturaer ennå/i)).toBeVisible();
    expect(screen.getByRole("button", { name: /download csv/i })).toBeVisible();
    expect(screen.queryByText("Current plan, renewal date, and upgrade path")).not.toBeInTheDocument();

    rerender(<VerevonWorkspaceSettingsPage section="sso" />);

    expect(screen.getByRole("heading", { name: /sso/i, level: 1 })).toBeVisible();
    expect(screen.getByRole("heading", { name: /connection test/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /run test/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /attribute mapping/i })).toBeVisible();
    expect(screen.queryByText("SSO provider selection and verified domain")).not.toBeInTheDocument();
  });
});
