import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { VelionAgentsPage } from "@/features/agents-v2/components/VelionAgentsPage";

describe("VelionAgentsPage", () => {
  afterEach(() => {
    window.history.pushState(null, "", "/");
  });

  it("shows the five agent role entry points on the first screen", () => {
    render(<VelionAgentsPage />);

    expect(screen.getByRole("heading", { name: /one agent system for the entire customer journey/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /service agent/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /sales agent/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /ecommerce agent/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /build your own chatbot/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /workflow builder/i })).toBeVisible();
  });

  it("opens activation-ready role pages for service, sales, and ecommerce", async () => {
    const user = userEvent.setup();
    render(<VelionAgentsPage />);

    await user.click(screen.getByRole("button", { name: /service agent/i }));

    expect(screen.getByRole("button", { name: /activate service agent/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /resolution queue/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /quality supervisor/i })).toBeVisible();
    expect(screen.getByText(/create support ticket/i)).toBeVisible();
    expect(screen.getByText(/verified resolution qa required/i)).toBeVisible();
    expect(screen.getByText(/zendesk-style verification/i)).toBeVisible();

    await setAgentRoute("/?agent=sales&feature=sales-lead-capture");

    expect(await screen.findByRole("button", { name: /activate sales agent/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /ai sdr journey/i })).toBeVisible();
    expect(screen.getByText(/pricing page trigger/i)).toBeVisible();
    expect(screen.getByLabelText(/piper engagement modes/i)).toBeVisible();
    expect(screen.getByText(/book meeting/i)).toBeVisible();
    expect(screen.getByText(/create crm handoff/i)).toBeVisible();

    await setAgentRoute("/?agent=sales&feature=sales-qualification");

    expect(await screen.findByLabelText(/visitor intelligence signals/i)).toBeVisible();

    await setAgentRoute("/?agent=ecommerce&feature=commerce-shopping");

    expect(await screen.findByRole("button", { name: /activate ecommerce agent/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /shopping assistant \+ support agent/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /product-page questions/i })).toBeVisible();
    expect(screen.getByText(/recommend products/i)).toBeVisible();

    await setAgentRoute("/?agent=ecommerce&feature=commerce-product-finder");

    expect(await screen.findByRole("heading", { name: /guided product finder/i })).toBeVisible();

    await setAgentRoute("/?agent=ecommerce&feature=commerce-store");

    expect(await screen.findByRole("heading", { name: /^store command center$/i, level: 2 })).toBeVisible();

    await setAgentRoute("/?agent=ecommerce&feature=commerce-brand");

    expect(await screen.findByRole("heading", { name: /ai persona \+ social/i })).toBeVisible();
    expect(screen.getByText(/ai persona drafted/i)).toBeVisible();
    expect(screen.getByText(/tone guide drafted/i)).toBeVisible();
  });

  it("opens the chatbot builder studio from the chatbot card", async () => {
    const user = userEvent.setup();
    render(<VelionAgentsPage />);

    await user.click(screen.getByRole("button", { name: /build your own chatbot/i }));

    expect(screen.getByRole("heading", { name: /^playground$/i, level: 1 })).toBeVisible();
    expect(screen.getByRole("heading", { name: /velion support agent/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /select update subscription add-on/i })).toBeVisible();
    expect(screen.getByRole("textbox", { name: /instructions system prompt/i })).toBeVisible();
    expect(screen.getByRole("combobox", { name: /model/i })).toBeVisible();
  });

  it("opens the workflow builder and configures a selected workflow node", async () => {
    const user = userEvent.setup();
    render(<VelionAgentsPage />);

    await user.click(screen.getByRole("button", { name: /workflow builder/i }));

    expect(screen.getByRole("heading", { name: /generate social media post/i })).toBeVisible();
    expect(screen.getByRole("heading", { name: /generate caption/i })).toBeVisible();
    expect(screen.getByRole("region", { name: /workflow canvas/i })).toBeVisible();
    expect(screen.getByRole("textbox", { name: /workflow prompt/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /select post on instagram workflow node/i })).toBeVisible();

    await user.click(screen.getByRole("button", { name: /select post on instagram workflow node/i }));

    expect(screen.getByRole("heading", { name: /post on instagram/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /test run/i })).toBeVisible();
  });
});

async function setAgentRoute(path: string) {
  await act(async () => {
    window.history.pushState(null, "", path);
    window.dispatchEvent(new Event("velion:agent-selection-change"));
  });
}
