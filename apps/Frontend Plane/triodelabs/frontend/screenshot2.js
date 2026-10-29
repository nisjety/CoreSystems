const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
  await page.goto('http://localhost:3002/', { waitUntil: 'networkidle' });
  await page.getByText('Egne', { exact: false }).first().scrollIntoViewIfNeeded();
  await page.waitForTimeout(600);
  await page.screenshot({ path: '/tmp/nav-check2.png' });
  await browser.close();
})();
