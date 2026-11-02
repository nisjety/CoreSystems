import { describe, expect, it, vi } from 'vitest';

// GSAP's ScrollTrigger reads window.matchMedia when the section module loads, and jsdom has none.
// Hoisted above the imports so the module can load; the copy and media data never use it.
vi.hoisted(() => {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: () => ({
      addEventListener() {},
      addListener() {},
      dispatchEvent: () => false,
      matches: false,
      media: '',
      onchange: null,
      removeEventListener() {},
      removeListener() {},
    }),
  });
});

import { loopStages } from './ProductLoopSection';

const stage = (step: string) => {
  const found = loopStages.find((item) => item.step === step);
  if (!found) throw new Error(`no product loop stage ${step}`);
  return found;
};

describe('product loop film stages', () => {
  it('02 Plan is a video film with the Norwegian copy from the plan', () => {
    const item = stage('02');
    expect(`${item.step} / ${item.state}`).toBe('02 / Plan');
    expect(item.kind).toBe('media');
    expect(item.title).toBe('Fra spørsmål til plan. Med kildene synlige.');
    expect(item.body).toBe('Verevon leser avtaler, ordre og e-post, viser hvor hvert funn kommer fra og lager et planutkast du godkjenner.');
    expect(item.media).toEqual({
      alt: expect.stringMatching(/Nordlys-leveransen/),
      kind: 'video',
      poster: '/product-scenes/02-plan-poster.webp',
      src: '/product-scenes/02-plan.mp4',
      webm: '/product-scenes/02-plan.webm',
    });
  });

  it('03 Undersøk is a video film with the Norwegian copy from the plan', () => {
    const item = stage('03');
    expect(`${item.step} / ${item.state}`).toBe('03 / Undersøk');
    expect(item.kind).toBe('media');
    expect(item.title).toBe('Finn årsaken. Se beviset. Godkjenn fiksen.');
    expect(item.body).toBe('Verevon undersøker parallelt, viser hva den fant i loggen og skjemaet, og foreslår en endring du kan teste før du sier ja.');
    expect(item.media).toEqual({
      alt: expect.stringMatching(/ordre 4821/),
      kind: 'video',
      poster: '/product-scenes/03-undersok-poster.webp',
      src: '/product-scenes/03-undersok.mp4',
      webm: '/product-scenes/03-undersok.webm',
    });
  });

  it('04 Prøv selv is the live interactive demo of the real dashboard, not a film', () => {
    const item = stage('04');
    expect(`${item.step} / ${item.state}`).toBe('04 / Prøv selv');
    expect(item.kind).toBe('live');
    expect(item.media).toBeUndefined();
    expect(item.title).toBe('Bruk Verevon selv. Rett her.');
    expect(item.body).toBe('Dette er det ekte dashbordet med fiktive data fra Fjordform AS. Spør, hent ut og følg opp, uten konto. Ingenting sendes, og ingenting forlater nettleseren din.');
    expect(item.live).toEqual({
      frameTitle: 'Verevon-demo: dashbordet med fiktive data for Fjordform AS',
      minAppHeight: 640,
      minAppWidth: 1100,
      poster: '/product-demo/poster-desktop.webp',
      posterAlt: expect.stringMatching(/fiktive data/),
      posterMobile: '/product-demo/poster-phone.webp',
      src: '/product-demo/index.html',
    });
  });

  it('keeps the composer as the last stage', () => {
    expect(loopStages.at(-1)).toMatchObject({ kind: 'composer', step: '05' });
  });
});
