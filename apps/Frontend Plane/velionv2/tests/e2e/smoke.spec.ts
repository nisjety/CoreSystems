import { expect, test } from "@playwright/test";

test("v1-parity auth, onboarding, dashboard, and chat surfaces render", async ({ page }) => {
  await page.route("**/api/v1/search/suggestions**", async (route) => {
    const url = new URL(route.request().url());
    const query = url.searchParams.get("q")?.trim() ?? "";
    const base = query.toLowerCase() === "find me" ? "Grocery store" : query.replace(/^find\s+me\s*/i, "");
    const suggestions = base
      ? [`Find me ${base}`, `Find me ${base} near me`, `Find me best ${base} options`]
      : [];

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        data: {
          configured: true,
          query,
          scope: "queries",
          suggestions: suggestions.map((text, index) => ({
            text,
            source: "query",
            collection: "queries",
            object: `test-${index}`,
          })),
        },
      }),
    });
  });

  await page.goto("/login");

  await expect(page.getByRole("heading", { name: /logg inn/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /logg inn med microsoft/i })).toBeVisible();
  await page.getByRole("button", { name: /select language/i }).click();
  await page.getByRole("button", { name: /english/i }).click();
  await expect(page.getByText(/language set to english/i)).toBeVisible();
  await page.getByLabel(/cookie-innstillinger/i).click();
  await expect(page.getByText(/nødvendige/i)).toBeVisible();
  await page
    .getByRole("dialog", { name: /cookie-innstillinger/i })
    .getByRole("button", { name: "Avvis alle", exact: true })
    .click();
  await expect(page.getByText(/informasjonskapsler på enheten/i)).toBeHidden();

  await page.goto("/onboarding");
  await expect(page.getByRole("heading", { name: /all set/i })).toBeVisible();
  await page.getByRole("button", { name: /continue setup/i }).click();
  await expect(page.getByRole("heading", { name: /company name/i })).toBeVisible();

  await page.goto("/dashboard");
  await expect(page.getByRole("heading", { name: /god (morgen|ettermiddag|kveld), ima/i })).toBeVisible();
  await expect(page.getByLabel(/message verevon/i)).toBeVisible();
  const sidebar = page.locator('aside[aria-label="Primary navigation"]');
  await expect(sidebar).toHaveCSS("width", "60px");
  await page.getByRole("button", { name: /expand sidebar/i }).click();
  await expect(sidebar).toHaveCSS("width", "320px");
  await expect(page.getByLabel(/filter sidebar section/i)).toBeVisible();
  await page.getByLabel(/filter sidebar section/i).fill("chat");
  await expect(page.getByRole("link", { name: /verevon chat/i })).toBeVisible();
  await page.getByLabel(/filter sidebar section/i).fill("");
  await sidebar.getByRole("button", { name: /^søk$/i }).click();
  await expect(page.getByRole("dialog", { name: /global search/i })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /open profile menu/i }).click();
  await expect(page.getByRole("link", { name: /^profile$/i })).toBeVisible();
  await page.keyboard.press("Escape");
  await sidebar.getByRole("button", { name: /collapse sidebar/i }).last().click();
  await expect(sidebar).toHaveCSS("width", "60px");
  await page.getByRole("button", { name: /toggle dark mode/i }).click();
  await expect(page.locator("html")).toHaveClass(/dark/);
  await page.getByRole("button", { name: /open global search/i }).first().click();
  await expect(page.getByRole("dialog", { name: /global search/i })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /quick messages/i }).click();
  await expect(page.getByText(/connect novu/i)).toBeVisible();
  await page.getByRole("button", { name: /calendar/i }).click();
  await expect(page.getByText(/no events for this day/i)).toBeVisible();
  const composer = page.locator(".verevon-home-composer");
  const composerCard = page.locator(".verevon-dashboard-composer-card").first();
  const modelButton = composer.getByRole("button", { name: /gpt-4o mini/i });
  await modelButton.click();
  const modelPanel = composer.locator(".verevon-popover").first();
  await expect(modelPanel).toBeVisible();
  const modelPanelBox = await modelPanel.boundingBox();
  const composerBox = await composer.boundingBox();
  expect(modelPanelBox).not.toBeNull();
  expect(composerBox).not.toBeNull();
  expect(modelPanelBox!.y + modelPanelBox!.height).toBeLessThanOrEqual(composerBox!.y);
  await page.getByRole("button", { name: /claude sonnet/i }).click();
  await expect(page.getByRole("button", { name: /claude sonnet/i })).toBeVisible();
  await page.getByLabel(/message verevon/i).fill("Monday");
  await expect(page.getByText(/schedule/i)).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByLabel(/message verevon/i)).toHaveValue(/Mon,/);
  await page.getByLabel(/message verevon/i).fill("");
  await page.getByRole("button", { name: /quick response/i }).click();
  await expect(page.locator("[data-mode-announcement='Quick response activated']")).toBeVisible();
  await page.getByRole("button", { name: /suggestions/i }).click();
  const suggestionsPanel = composer.locator(".verevon-popover").first();
  await expect(suggestionsPanel).toBeVisible();
  const suggestionsPanelBox = await suggestionsPanel.boundingBox();
  const composerCardBox = await composerCard.boundingBox();
  expect(suggestionsPanelBox).not.toBeNull();
  expect(composerCardBox).not.toBeNull();
  expect(suggestionsPanelBox!.y + suggestionsPanelBox!.height).toBeLessThanOrEqual(composerCardBox!.y);
  await page.getByText(/svarutkast/i).click();
  await page.getByRole("button", { name: /deep search/i }).click();
  await expect(page.getByRole("button", { name: /deep search/i })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: /send message/i }).click();
  await expect(page.locator(".verevon-fade-up").filter({ hasText: /kort svarutkast/i }).first()).toBeVisible();
  await page.getByRole("button", { name: /^voice mode$/i }).click();
  await expect(page.getByRole("dialog", { name: /voice mode/i })).toBeVisible();
  await page.getByRole("button", { name: /close voice mode/i }).click();
  const historyButton = composer.getByRole("button", { name: /historikk/i });
  const historyButtonBox = await historyButton.boundingBox();
  await historyButton.click();
  const historyPanel = page.locator("[data-composer-floating-panel='true']").first();
  await expect(historyPanel).toBeVisible();
  await expect(page.getByText(/view all conversations/i)).toBeVisible();
  const historyPanelBox = await historyPanel.boundingBox();
  expect(historyButtonBox).not.toBeNull();
  expect(historyPanelBox).not.toBeNull();
  expect(historyPanelBox!.y + historyPanelBox!.height).toBeLessThanOrEqual(historyButtonBox!.y);
  await page.keyboard.press("Escape");
  const settingsButton = composer.getByRole("button", { name: /innstillinger/i });
  const settingsButtonBox = await settingsButton.boundingBox();
  await settingsButton.click();
  const settingsPanel = page.locator("[data-composer-floating-panel='true']").first();
  await expect(settingsPanel).toBeVisible();
  await expect(page.getByText(/add files or photos/i)).toBeVisible();
  await expect(page.getByText(/voice language/i)).toBeVisible();
  await settingsPanel.getByRole("button", { name: /skills/i }).click();
  await expect(settingsPanel.getByText(/^skills$/i)).toBeVisible();
  await settingsPanel.getByRole("button", { name: /skills/i }).click();
  const settingsPanelBox = await settingsPanel.boundingBox();
  expect(settingsButtonBox).not.toBeNull();
  expect(settingsPanelBox).not.toBeNull();
  expect(settingsPanelBox!.x).toBeGreaterThanOrEqual(settingsButtonBox!.x + settingsButtonBox!.width);
  await page.keyboard.press("Escape");
  const main = page.getByRole("main");
  await main.getByRole("button", { name: /^søk$/i }).click();
  await page.getByRole("textbox", { name: /søk i selskapets kunnskap/i }).fill("personvern");
  await expect(page.getByRole("button", { name: /find me personvern near me/i })).toBeVisible();
  await expect(page.getByText(/søket er klart/i)).toBeHidden();
  await main.getByRole("button", { name: /^søk$/i }).last().click();
  await expect(page.getByText(/søket er klart/i)).toBeVisible();
  await page.getByRole("textbox", { name: /søk i selskapets kunnskap/i }).fill("Find me");
  await expect(page.getByText(/søket er klart/i)).toBeHidden();
  await expect(page.getByRole("button", { name: "Find me Grocery store", exact: true })).toBeVisible();
  await page.getByRole("button", { name: /^kunnskap$/i }).click();
  await page.getByRole("button", { name: /^vis$/i }).click();
  await expect(page.getByText(/koble kilde/i)).toBeVisible();
  await page.getByRole("button", { name: /vis neste kortside/i }).click();
  await expect(page.getByText(/^datakilder$/i)).toBeVisible();

  await page.goto("/agents");
  const agentsSidebar = page.locator('aside[aria-label="Primary navigation"]');
  const agentSelector = agentsSidebar.getByLabel(/select agent type/i);
  await expect(agentsSidebar).toHaveCSS("width", "320px");
  await expect(agentsSidebar.getByText(/^Agents$/i)).toBeVisible();
  await expect(agentSelector).toContainText("All roles");
  await expect(page.getByRole("heading", { name: /one agent system for the entire customer journey/i, level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /service agent/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /sales agent/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /ecommerce agent/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /build your own chatbot/i })).toBeVisible();
  await agentSelector.click();
  await page.getByRole("menuitemradio", { name: /^Sales$/i }).click();
  await expect(page).toHaveURL(/agent=sales/);
  await expect(page).toHaveURL(/feature=sales-lead-capture/);
  await expect(agentsSidebar.getByRole("navigation", { name: /agent feature tabs/i })).toBeVisible();
  await expect(agentsSidebar.getByRole("button", { name: /^Qualification Drift-style/i })).toBeVisible();
  await expect(page.getByRole("heading", { name: /sales agent intent capture workspace/i, level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: /lead qualification preview/i })).toBeVisible();
  await agentsSidebar.getByRole("button", { name: /^Qualification Drift-style/i }).click();
  await expect(page).toHaveURL(/feature=sales-qualification/);
  await expect(page.getByRole("heading", { name: /qualify leads before booking a meeting/i, level: 1 })).toBeVisible();
  await agentSelector.click();
  await page.getByRole("menuitemradio", { name: /^Chatbot$/i }).click();
  await expect(page).toHaveURL(/agent=chatbot/);
  await expect(page).toHaveURL(/view=playground/);
  await expect(agentsSidebar.getByRole("navigation", { name: /chatbot builder navigation/i })).toBeVisible();
  await expect(agentsSidebar.getByText(/runtime checks live/i)).toBeVisible();
  await expect(page.getByRole("heading", { name: /^playground$/i, level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /select update subscription add-on/i })).toBeVisible();
  await agentsSidebar.getByRole("button", { name: /^Fine-tuning$/i }).click();
  await expect(page).toHaveURL(/view=data-sources/);
  await expect(page.getByRole("heading", { name: /^fine-tuning$/i, level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /learn more/i })).toBeVisible();
  await expect(page.getByRole("button", { name: /drag & drop fine-tuning datasets/i })).toBeVisible();
  await agentsSidebar.getByRole("button", { name: /^Tools$/i }).click();
  await expect(page).toHaveURL(/view=actions/);
  await expect(page.getByRole("heading", { name: /^tools$/i, level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /create tool/i })).toBeVisible();
  await agentSelector.click();
  await page.getByRole("menuitemradio", { name: /^Workflow builder$/i }).click();
  await expect(page).toHaveURL(/agent=workflow/);
  await expect(agentsSidebar.getByRole("complementary", { name: /workflow tools/i })).toBeVisible();
  await expect(page.getByRole("heading", { name: /generate social media post/i, level: 1 })).toBeVisible();
  await expect(page.getByRole("region", { name: /workflow canvas/i })).toBeVisible();
  await page.goto("/agents/chatbots/new");
  await expect(agentSelector).toContainText("Chatbot");
  await expect(page).toHaveURL(/\/agents\/chatbots\/new$/);
  await expect(agentsSidebar.getByRole("navigation", { name: /chatbot builder navigation/i })).toBeVisible();
  await expect(page.getByRole("heading", { name: /^playground$/i, level: 1 })).toBeVisible();

  await page.goto("/chat");
  await expect(page.getByRole("heading", { name: /hva kan jeg hjelpe med/i, level: 1 })).toBeVisible();
  await expect(page.getByLabel(/message verevon/i)).toBeVisible();
  await expect(page.getByRole("button", { name: /use quick prompt: create slides/i })).toBeVisible();
  await page.getByRole("button", { name: /show more quick prompts/i }).click();
  await expect(page.getByRole("menuitem", { name: /^code$/i })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByText(/ChatGPT-style conversation with Manus-style task feedback/i)).toHaveCount(0);
  const chatSidebar = page.locator('aside[aria-label="Primary navigation"]');
  await expect(chatSidebar).toHaveCSS("width", "60px");
  await page.getByRole("button", { name: /expand sidebar/i }).click();
  await expect(chatSidebar).toHaveCSS("width", "320px");
  await expect(page.getByLabel(/search conversations/i)).toBeVisible();
  await expect(page.getByRole("navigation", { name: /chat conversations/i })).toBeVisible();
  await page.getByLabel(/message verevon/i).fill("Draft a response plan for today's inbox");
  await page.getByRole("button", { name: /send message/i }).click();
  await expect(page.locator("article").filter({ hasText: /draft a response plan for today's inbox/i }).first()).toBeVisible();
  await expect(page.getByText(/prompt received/i)).toBeVisible();
  await expect(page.getByText(/model gateway/i)).toBeVisible();
  await page.getByLabel(/search conversations/i).fill("response plan");
  await expect(page.getByRole("button", { name: /draft a response plan/i })).toBeVisible();
});
