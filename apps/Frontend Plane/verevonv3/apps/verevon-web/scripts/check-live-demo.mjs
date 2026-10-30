#!/usr/bin/env node
// Static check of the stage 04 live demo wiring on the site (verevon-web).
//
// Reads source text only: no network, no browser, no build. It fails when the wiring breaks the
// contract in plans/product-demo/PLAN_2026-10-11.md (iframe src and sandbox, no origin check, no
// pointer-events or inert on the iframe, timing, the timeline hold, the removed React mock and
// the fictional-world rule). Built assets that are produced later (the demo bundle and the posters)
// are warnings; pass --strict to fail on them as well.
//
// Run: node scripts/check-live-demo.mjs [--strict]   (from apps/verevon-web)

import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const siteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const strict = process.argv.includes("--strict");
const sections = "src/components/home/sections";
const bundle = "public/product-demo";

const failures = [];
const warnings = [];
const read = (relative) => readFileSync(join(siteRoot, relative), "utf8");
const expect = (condition, message) => {
  if (!condition) failures.push(message);
};
const warnIf = (condition, message) => {
  if (!condition) warnings.push(message);
};

const section = read(`${sections}/ProductLoopSection.tsx`);
const liveDemo = read(`${sections}/product-loop-live-demo.ts`);
const component = read(`${sections}/ProductLoopLiveDemo.tsx`);
const progress = read(`${sections}/product-loop-progress.ts`);
const liveCode = `${liveDemo}\n${component}`;

// The React mock of stage 04 is gone from the section and from the tree.
expect(
  !/product-demo\/product-demo|ResponsiveDemo|LoopDemoPreview|\bProductDemo\b/.test(section),
  "ProductLoopSection.tsx still uses the React mock (ResponsiveDemo, LoopDemoPreview or ProductDemo)",
);
// Older, untracked demo experiments may be present in a working checkout. They are safe to
// retain while unused; this check must not require deleting someone else's local files.
if (existsSync(join(siteRoot, "src/components/home/product-demo"))) {
  console.log("INFO unused local src/components/home/product-demo retained");
}
if (existsSync(join(siteRoot, "src/app/dev/product-demo"))) {
  console.log("INFO unused local src/app/dev/product-demo retained");
}

// Stage 04 is the live demo.
expect(/kind:\s*"live"/.test(section), 'stage 04 is not kind "live"');
expect(/live:\s*PRODUCT_LOOP_LIVE_DEMO/.test(section), "stage 04 does not carry PRODUCT_LOOP_LIVE_DEMO");

// The iframe contract: the bundle entry on the same origin, and a sandbox of allow-scripts only.
expect(/src:\s*"\/product-demo\/index\.html"/.test(liveDemo), "the iframe src is not /product-demo/index.html");
expect(/LIVE_DEMO_SANDBOX\s*=\s*"allow-scripts"/.test(liveDemo), "LIVE_DEMO_SANDBOX is not exactly allow-scripts");
expect(
  !/allow-same-origin|allow-forms|allow-popups|allow-top-navigation|allow-downloads/.test(liveCode),
  "a sandbox token beyond allow-scripts appears in the live demo code",
);
const iframeTag = component.match(/<iframe[\s\S]*?\/>/)?.[0] ?? "";
expect(iframeTag.length > 0, "the live demo component renders no iframe");
expect(
  !/pointer-events|inert/.test(iframeTag),
  "the iframe is pointer-events:none or inert: the shield covers it instead (un-gating stops wheel scrolling)",
);

// Messages are checked by source and envelope. The sandboxed frame has an opaque origin, so an origin check would always fail.
expect(!/event\.origin/.test(liveCode), "the live demo checks event.origin; an opaque (sandboxed) origin reports 'null'");
expect(/readDemoMessage\(/.test(component), "the component does not check messages with readDemoMessage");

// Timing and copy anchors of the plan.
expect(/LIVE_DEMO_READY_TIMEOUT_MS\s*=\s*15_?000/.test(liveDemo), "the load timeout is not 15 s");
expect(
  !/location\.replace\(|location\.href\s*=/.test(liveCode),
  "the site navigates the demo frame itself: a second navigation can cut the demo's own reset short",
);
expect(/LIVE_DEMO_BAR_HEIGHT\s*=\s*44\b/.test(liveDemo), "the stage bar is not 44 px");
expect(/Demo · fiktive data/.test(liveDemo), "the honest label 'Demo · fiktive data' is missing");

// The timeline hold goes through the exported constants, and the pinned height follows from it.
expect(/PRODUCT_LOOP_STAGE04_HOLD\s*=\s*0\.5\b/.test(progress), "the stage 04 hold is not 0.5");
expect(/h-\[436svh\]/.test(section), "the section is not pinned at 436svh");

// Fictional world only, in the visitor-visible strings of the live demo.
expect(!/Aquatiq|Velion|\bIma\b/.test(liveCode), "a real company or person name appears in the live demo code");
const addresses = liveCode.match(/[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g) ?? [];
expect(
  addresses.every((address) => address.endsWith("@fjordform.example")),
  `an e-mail address outside @fjordform.example appears in the live demo code: ${addresses.join(", ")}`,
);

// Built assets: produced by the demo build and the orchestrator, so warnings until they exist.
warnIf(existsSync(join(siteRoot, bundle, "index.html")), `${bundle}/index.html is missing: run the demo build in the SPA before the stage can load`);
warnIf(existsSync(join(siteRoot, bundle, "poster-desktop.webp")), `${bundle}/poster-desktop.webp is missing: the stage shows the CSS fallback`);
warnIf(existsSync(join(siteRoot, bundle, "poster-phone.webp")), `${bundle}/poster-phone.webp is missing: the phone card shows the CSS fallback`);

for (const message of failures) console.error(`FAIL ${message}`);
for (const message of warnings) console.warn(`WARN ${message}`);
const failed = failures.length > 0 || (strict && warnings.length > 0);
if (!failed) console.log(`live demo wiring OK (${warnings.length} warning(s))`);
process.exit(failed ? 1 : 0);
