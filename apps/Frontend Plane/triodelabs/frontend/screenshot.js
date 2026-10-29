const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
  await page.goto('http://localhost:3002/', { waitUntil: 'networkidle' });
  await page.evaluate(() => window.scrollTo(0, 900));
  await page.waitForTimeout(500);
  await page.screenshot({ path: '/tmp/nav-check.png' });
  await browser.close();
})();
