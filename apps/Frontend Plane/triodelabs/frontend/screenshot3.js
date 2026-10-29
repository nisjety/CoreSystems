const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
  await page.goto('http://localhost:3002/', { waitUntil: 'networkidle' });
  const el = await page.getByText('Egne', { exact: false }).first();
  const box = await el.boundingBox();
  await page.evaluate((y) => window.scrollBy(0, y - 80), box.y);
  await page.waitForTimeout(600);
  await page.screenshot({ path: '/tmp/nav-check3.png' });
  await browser.close();
})();
