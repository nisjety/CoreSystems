// This function runs in a private Chromium JavaScript world. Page scripts cannot
// replace its native DOM methods, choose target IDs, or access its state.
export function evaluatePage(operation, input) {
  const state = globalThis.__quarryDesktop ??= { targets: new Map() };
  const clean = (value, length = 200) => String(value ?? '').replace(/\s+/g, ' ').trim().slice(0, length);
  const name = (element) => clean(
    element.getAttribute('aria-label') ||
    (element.getAttribute('aria-labelledby') || '').split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent).filter(Boolean).join(' ') ||
    Array.from(element.labels ?? []).map((label) => label.textContent).join(' ') ||
    element.innerText || element.textContent || element.getAttribute('title') ||
    element.querySelector('img[alt]')?.getAttribute('alt') ||
    element.getAttribute('alt') || element.getAttribute('placeholder') || element.value,
  );
  const visible = (element) => Boolean(element?.isConnected && element.getClientRects().length &&
    !element.closest('[inert],[aria-hidden="true"],[hidden]') &&
    getComputedStyle(element).visibility === 'visible');
  const disabled = (element) => element.matches(':disabled,[aria-disabled="true"]');
  const roleOf = (element) => {
    if (element.matches('a[href]')) return 'link';
    if (element.matches('input[type=checkbox],input[type=radio]')) return element.type;
    if (element.matches('input[type=search]')) return 'searchbox';
    return element.getAttribute('role') || (element.matches('button,summary') ? 'button' : 'textbox');
  };
  const searchField = (element) => {
    if (!element.matches('input[type=search],input[type=text],input:not([type])') ||
        element.readOnly || disabled(element) || element.form?.method?.toLowerCase() === 'post') return false;
    return element.type === 'search' || element.getAttribute('role') === 'searchbox' ||
      Boolean(element.closest('[role=search]')) || /\b(søk|search|rechercher|suche)\b/i.test(name(element));
  };
  const targetElement = (element) => {
    const label = Array.from(element.labels ?? []).find(visible);
    if (label && element.matches('input[type=checkbox],input[type=radio]')) return label;
    if (visible(element) && element.getBoundingClientRect().width > 1 &&
        element.getBoundingClientRect().height > 1) return element;
    return label;
  };
  const token = () => `t${Array.from(crypto.getRandomValues(new Uint8Array(8)),
    (byte) => byte.toString(16).padStart(2, '0')).join('')}`;

  if (operation === 'consentFrames') {
    return Array.from(document.querySelectorAll('[aria-modal=true] iframe,[role=dialog] iframe,dialog[open] iframe'))
      .filter((frame) => /cookie|informasjonskapsl|samtykke|consent/i.test(frame.title))
      .slice(0, 3).map((frame) => {
        const rect = frame.getBoundingClientRect();
        return { url: frame.src, x: rect.left + frame.clientLeft, y: rect.top + frame.clientTop,
          width: frame.clientWidth, height: frame.clientHeight };
      });
  }

  if (operation === 'consentChoice') {
    const reject = /^(avvis alle|avslå alle|avvis valgfrie|kun nødvendige|bare nødvendige|reject all|decline all|reject optional cookies|only necessary cookies|essential cookies only)$/i;
    const manage = /^(tilpass eller avvis|customi[sz]e or reject|manage or reject)$/i;
    const choices = Array.from(document.querySelectorAll('button,[role=button]')).filter((button) => visible(button) && !disabled(button));
    const button = choices.find((item) => reject.test(name(item))) ?? choices.find((item) => manage.test(name(item)));
    if (!button) return null;
    button.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    const rect = button.getBoundingClientRect();
    const x = Math.round(rect.left + rect.width / 2);
    const y = Math.round(rect.top + rect.height / 2);
    return button.contains(document.elementFromPoint(x, y)) ? { x, y, reject: reject.test(name(button)) } : null;
  }

  if (operation === 'snapshot') {
    state.targets.clear();
    const controls = document.querySelectorAll(
      'button,summary,input[type=checkbox],input[type=radio],input[type=search],input[type=text],input:not([type]),' +
      '[role=button],[role=checkbox],[role=radio],[role=switch],[role=tab],[role=option],[role=searchbox]',
    );
    const links = document.querySelectorAll('a[href]');
    const targets = [];
    for (const element of [...controls, ...links]) {
      if (targets.length >= 100) break;
      const target = targetElement(element);
      if (!target || disabled(element)) continue;
      const role = roleOf(element);
      if (role === 'textbox' && !searchField(element)) continue;
      const label = name(element);
      if (!label) continue;
      const href = role === 'link' ? element.href : null;
      if (href && (!/^https?:\/\//.test(href) || href.length > 4096)) continue;
      const id = token();
      state.targets.set(id, { element, role, label, href, fieldName: element.name,
        fieldValue: ['checkbox', 'radio'].includes(role) ? element.value : undefined });
      const info = { id, role, label };
      if (href) info.href = href;
      if (['checkbox', 'radio', 'switch', 'option', 'tab'].includes(role)) {
        info.checked = element.checked ?? (element.getAttribute('aria-checked') === 'true' ||
          element.getAttribute('aria-selected') === 'true');
      }
      if (searchField(element)) info.value = clean(element.value);
      targets.push(info);
    }
    return {
      heading: clean(document.querySelector('main h1,h1,[role=heading][aria-level="1"]')?.innerText) || null,
      text: (document.querySelector('main')?.innerText ?? document.body?.innerText ?? '').trim().slice(0, 12_000),
      targets,
    };
  }

  if (operation === 'target') {
    const found = state.targets.get(input.id);
    if (!found) return { unavailable: 'unknown' };
    if (!found.element.isConnected) {
      // Hydrating applications may replace the original node between reading
      // it and clicking. Only rebind to a single element with the same identity.
      const sameTarget = (candidate) => roleOf(candidate) === found.role && name(candidate) === found.label &&
        (!found.href || candidate.href === found.href) && candidate.name === found.fieldName &&
        (found.fieldValue === undefined || candidate.value === found.fieldValue);
      const identified = found.element.id && document.getElementById(found.element.id);
      if (identified && sameTarget(identified)) found.element = identified;
      else {
        const replacements = Array.from(document.getElementsByTagName(found.element.localName)).filter(sameTarget);
        if (replacements.length !== 1) return { unavailable: 'replaced' };
        [found.element] = replacements;
      }
    }
    if (disabled(found.element)) return { unavailable: 'disabled' };
    const { element, role } = found;
    const target = targetElement(element);
    if (!target) return { unavailable: 'hidden' };
    const label = name(element);
    const destructive = /(?:\b(?:send|submit|publish|delete|remove|buy|order|pay|login|sign\s*in|betal|bestill|kjøp|slett|fjern|publiser|logg\s*inn)\b)/i;
    const allowedButton = (role === 'button' || role === 'option') &&
      (!element.form || element.type === 'button') &&
      (element.matches('summary,[aria-expanded],[aria-haspopup]') ||
        /(?:\b(?:search|søk|filter|vis|velg|sorter|neste|next|tilbake|back|more|mer|lukk|close|avvis|reject|decline)\b)/i.test(label));
    if (input.kind === 'fill' ? !searchField(element) :
        destructive.test(label) || (role === 'link' ? element.hasAttribute('download') :
          !['checkbox', 'radio', 'tab', 'switch'].includes(role) && !allowedButton)) return { unavailable: 'policy' };

    target.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
    const href = role === 'link' ? element.href : null;
    for (const bounds of target.getClientRects()) {
      const left = Math.max(bounds.left, 0);
      const right = Math.min(bounds.right, innerWidth);
      const top = Math.max(bounds.top, 0);
      const bottom = Math.min(bounds.bottom, innerHeight);
      if (right <= left || bottom <= top) continue;
      for (const [horizontal, vertical] of [[0.5, 0.5], [0.25, 0.25], [0.75, 0.25], [0.25, 0.75], [0.75, 0.75]]) {
        const x = Math.round(left + (right - left) * horizontal);
        const y = Math.round(top + (bottom - top) * vertical);
        const hit = document.elementFromPoint(x, y);
        if (target.contains(hit)) return { x, y, href };
      }
    }
    // A rendered anchor still supplies a navigable public URL. Never click an
    // obstructing element or turn an unverified control into a click.
    return href ? { href, covered: true } : { unavailable: 'covered' };
  }

  if (operation === 'focus') return searchField(document.activeElement ?? document.documentElement);

  if (operation === 'cursor') {
    if (!document.documentElement) return;
    if (!state.cursor?.isConnected) {
      const marker = document.createElement('div');
      marker.setAttribute('aria-hidden', 'true');
      marker.style.cssText = 'all:initial;position:fixed;left:0;top:0;z-index:2147483647;pointer-events:none';
      const shadow = marker.attachShadow({ mode: 'closed' });
      const arrow = document.createElement('span');
      arrow.style.cssText = 'position:absolute;top:0;left:0;background:#a64d32;width:15px;height:18px;' +
        'clip-path:polygon(0 0,100% 61%,61% 70%,49% 100%);filter:drop-shadow(0 1px 1px #fff)';
      const label = document.createElement('span');
      label.textContent = 'Verevon';
      label.style.cssText = 'position:absolute;top:13px;left:13px;white-space:nowrap;font:600 11px system-ui;' +
        'background:#a64d32;color:white;border:1px solid white;padding:2px 6px;border-radius:6px';
      shadow.append(arrow, label);
      (document.body ?? document.documentElement).append(marker);
      state.cursor = marker;
    }
    state.cursor.style.transform = `translate(${input.x}px,${input.y}px)`;
    if (input.release) {
      const marker = state.cursor;
      document.addEventListener('pointermove', () => marker.remove(), { once: true, capture: true });
    }
  }
}
