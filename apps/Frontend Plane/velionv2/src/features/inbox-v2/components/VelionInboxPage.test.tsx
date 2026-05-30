import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { VelionInboxPage } from "@/features/inbox-v2/components/VelionInboxPage";

const ticket = {
  id: 42,
  number: "54172",
  title: "Order marked delivered but missing",
  state: { id: 2, name: "open" },
  priority: { id: 3, name: "high" },
  group: { id: 1, name: "Support" },
  owner: null,
  customer: { id: 7, firstname: "Maya", lastname: "Solberg", email: "maya@example.com" },
  tags: ["delivery"],
  created_at: "2026-05-24T09:35:00.000Z",
  updated_at: "2026-05-26T08:58:00.000Z",
};

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: init.status ?? 200,
    headers: { "Content-Type": "application/json" },
  });
}

describe("VelionInboxPage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the recreated empty inbox state when support is unavailable", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse({ error: "support_not_configured" }, { status: 503 }));

    render(<VelionInboxPage />);

    expect(screen.getByRole("heading", { name: "Inbox" })).toBeVisible();
    expect(screen.getByText("Select all")).toBeVisible();
    expect(screen.getByRole("button", { name: /sort conversations/i })).toBeVisible();
    expect(screen.getByText(/select a ticket to view the conversation/i)).toBeVisible();
    expect(screen.getByRole("complementary", { name: /ai and customer context/i })).toBeVisible();
    await waitFor(() => expect(screen.getByText(/failed to load tickets: 503/i)).toBeVisible());
  });

  it("loads tickets and opens the selected conversation", async () => {
    const user = userEvent.setup();
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = String(input);
      if (url.startsWith("/api/support/tickets?")) return jsonResponse({ tickets: [ticket], total: 1 });
      if (url === "/api/support/agents" || url === "/api/support/groups") return jsonResponse([]);
      if (url === "/api/support/tickets/42/articles") {
        return jsonResponse({
          articles: [
            {
              id: 1,
              ticket_id: 42,
              body: "My package is missing.",
              sender: "Customer",
              from: "Maya Solberg",
              internal: false,
              created_at: "2026-05-26T08:58:00.000Z",
            },
          ],
        });
      }
      return jsonResponse({});
    });

    render(<VelionInboxPage />);

    const ticketList = await screen.findByRole("list", { name: /tickets/i });
    await user.click(await within(ticketList).findByRole("button", { name: /order marked delivered but missing/i }));

    expect(await screen.findByRole("heading", { name: /order marked delivered but missing/i })).toBeVisible();
    expect(screen.getByText(/my package is missing/i)).toBeVisible();
    expect(screen.getByPlaceholderText(/reply to maya solberg/i)).toBeVisible();
  });
});
