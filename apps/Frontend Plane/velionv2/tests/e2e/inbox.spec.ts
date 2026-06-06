import { expect, test } from "@playwright/test";

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

test("inbox renders support conversation data and sends a reply", async ({ page }) => {
  await page.setExtraHTTPHeaders({
    "x-playwright-auth-user-id": "u_playwright",
    "x-playwright-auth-email": "ima@example.com",
    "x-playwright-auth-name": "Ima Agent",
    "x-playwright-org-id": "org_playwright",
  });

  await page.route("**/api/support/tickets?**", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ tickets: [ticket], total: 1 }),
    });
  });

  await page.route("**/api/support/agents", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify([{ id: 11, firstname: "Ima", lastname: "Agent", email: "ima@example.com" }]),
    });
  });

  await page.route("**/api/support/groups", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify([{ id: 1, name: "Support" }]),
    });
  });

  await page.route("**/api/support/macros", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify([{ id: 1, name: "Shipping update" }]),
    });
  });

  await page.route("**/api/support/customers/*/context?**", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ shopify: null, stripe: null }),
    });
  });

  await page.route("**/api/support/tickets/42/articles", async (route) => {
    if (route.request().method() === "POST") {
      const payload = route.request().postDataJSON() as { body?: string; internal?: boolean };
      await route.fulfill({
        status: 201,
        contentType: "application/json",
        body: JSON.stringify({
          article: {
            id: 2,
            ticket_id: 42,
            body: payload.body ?? "",
            sender: "Agent",
            from: "Ima Agent",
            internal: Boolean(payload.internal),
            created_at: "2026-05-26T09:05:00.000Z",
          },
        }),
      });
      return;
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
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
      }),
    });
  });

  await page.route("**/api/support/tickets/42/sentiment", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ sentiment: "frustrated", score: 86 }),
    });
  });

  await page.route("**/api/support/tickets/42/quick-replies", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ options: ["Thanks for the context. I am checking the delivery trail now."] }),
    });
  });

  await page.goto("/inbox");

  await expect(page.getByRole("heading", { name: "Inbox", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: /order marked delivered but missing/i }).click();

  await expect(page.getByText(/my package is missing/i)).toBeVisible();
  await expect(page.getByText("Very Frustrated")).toBeVisible();

  await page.getByRole("button", { name: "AI assist", exact: true }).click();
  await expect(page.getByLabel(/reply to maya solberg/i)).toHaveValue(/checking the delivery trail/i);

  await page.getByLabel(/reply to maya solberg/i).fill("We are checking the delivery trail and will update you shortly.");
  await page.getByRole("button", { name: "Send", exact: true }).click();

  await expect(page.getByText(/reply sent/i)).toBeVisible();
  await expect(page.getByText(/we are checking the delivery trail/i)).toBeVisible();
});
