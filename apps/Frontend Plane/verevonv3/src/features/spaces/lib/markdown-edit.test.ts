import { describe, expect, it } from 'vitest';

import { applyFormat, markdownShortcut, type MarkdownEdit } from './markdown-edit';

// WS15-S3: the composer's formatting transforms. Each one takes a value and a selection and returns the
// new value and selection, so the same function serves the toolbar buttons and the Ctrl/Cmd shortcuts.

/** The text the edit leaves selected: what the person sees highlighted after the format. */
const selected = (edit: MarkdownEdit) => edit.value.slice(edit.selStart, edit.selEnd);

describe('applyFormat (WS15-S3)', () => {
  describe('bold, italic and code', () => {
    it('wraps the selected word and keeps the selection on it', () => {
      const edit = applyFormat('hello world', 6, 11, 'bold');
      expect(edit.value).toBe('hello **world**');
      expect(selected(edit)).toBe('world');
    });

    it('uses underscores for italic and backticks for code', () => {
      expect(applyFormat('a b', 2, 3, 'italic').value).toBe('a _b_');
      expect(applyFormat('run it', 4, 6, 'code').value).toBe('run `it`');
    });

    it.each([
      ['bold', '**'],
      ['italic', '_'],
      ['code', '`'],
    ] as const)('%s unwraps the marks it wrapped, when they sit outside the selection', (kind, mark) => {
      const wrapped = applyFormat('say word now', 4, 8, kind);
      expect(wrapped.value).toBe(`say ${mark}word${mark} now`);
      const unwrapped = applyFormat(wrapped.value, wrapped.selStart, wrapped.selEnd, kind);
      expect(unwrapped.value).toBe('say word now');
      expect(selected(unwrapped)).toBe('word');
    });

    it('unwraps when the marks sit inside the selection', () => {
      const edit = applyFormat('hello **world**', 6, 15, 'bold');
      expect(edit.value).toBe('hello world');
      expect(selected(edit)).toBe('world');
    });

    it('inserts both marks with the caret between them when nothing is selected', () => {
      const bold = applyFormat('ab', 1, 1, 'bold');
      expect(bold.value).toBe('a****b');
      expect(bold.selStart).toBe(3);
      expect(bold.selEnd).toBe(3);

      const italic = applyFormat('ab', 1, 1, 'italic');
      expect(italic.value).toBe('a__b');
      expect(italic.selStart).toBe(2);
      expect(italic.selEnd).toBe(2);
    });
  });

  describe('link', () => {
    it('labels the selected text and selects the https:// placeholder to type over', () => {
      const edit = applyFormat('see docs', 4, 8, 'link');
      expect(edit.value).toBe('see [docs](https://)');
      expect(selected(edit)).toBe('https://');
    });

    it('labels a selected URL and puts the caret in the empty label', () => {
      const edit = applyFormat('see https://example.test/a', 4, 26, 'link');
      expect(edit.value).toBe('see [](https://example.test/a)');
      expect(edit.selStart).toBe(5);
      expect(edit.selEnd).toBe(5);
    });

    it('treats a selected mailto address as a link target too', () => {
      const edit = applyFormat('mailto:kari@example.test', 0, 24, 'link');
      expect(edit.value).toBe('[](mailto:kari@example.test)');
      expect(edit.selStart).toBe(1);
    });

    it('puts the caret in the label when nothing is selected', () => {
      const edit = applyFormat('ab', 1, 1, 'link');
      expect(edit.value).toBe('a[](https://)b');
      expect(edit.selStart).toBe(2);
      expect(edit.selEnd).toBe(2);
    });
  });

  describe('bullet and quote', () => {
    it('prefixes every line the selection touches', () => {
      const edit = applyFormat('one\ntwo', 0, 7, 'bullet');
      expect(edit.value).toBe('- one\n- two');
      expect(selected(edit)).toBe('one\n- two');
    });

    it('does not reach the line after a selection that ends right after its newline', () => {
      const edit = applyFormat('one\ntwo\nthree', 0, 4, 'bullet');
      expect(edit.value).toBe('- one\ntwo\nthree');
    });

    it('prefixes the line the caret is on when nothing is selected', () => {
      const edit = applyFormat('one\ntwo', 5, 5, 'quote');
      expect(edit.value).toBe('one\n> two');
    });

    it('removes the prefix when every touched line already carries it', () => {
      expect(applyFormat('> a\n> b', 0, 7, 'quote').value).toBe('a\nb');
      expect(applyFormat('- a\n- b', 0, 7, 'bullet').value).toBe('a\nb');
    });

    it('adds the prefix to every line when only some of them carry it', () => {
      expect(applyFormat('- a\nb', 0, 5, 'bullet').value).toBe('- - a\n- b');
    });

    it('prefixes the first line when the value begins with a line break and the selection starts at 0', () => {
      // The caret is on the empty first line, so that is the line that gets the bullet.
      expect(applyFormat('\nabc', 0, 0, 'bullet').value).toBe('- \nabc');
    });
  });

  describe('selection handling', () => {
    it('clamps a selection that runs past either end of the value', () => {
      expect(applyFormat('abc', -5, 99, 'bold').value).toBe('**abc**');
      const past = applyFormat('abc', 10, 12, 'bold');
      expect(past.value).toBe('abc****');
      expect(past.selStart).toBe(5);
    });

    it('reads a reversed selection as the same range', () => {
      const edit = applyFormat('hello world', 11, 6, 'bold');
      expect(edit.value).toBe('hello **world**');
      expect(selected(edit)).toBe('world');
    });
  });
});

describe('markdownShortcut (WS15-S3)', () => {
  it('maps b, i and k to bold, italic and link, in either case', () => {
    expect(markdownShortcut('b')).toBe('bold');
    expect(markdownShortcut('B')).toBe('bold');
    expect(markdownShortcut('i')).toBe('italic');
    expect(markdownShortcut('K')).toBe('link');
  });

  it('maps no other key, so code, bullet and quote have no shortcut', () => {
    for (const key of ['c', 'q', 'l', 'e', 'Enter', 'bb', '']) {
      expect(markdownShortcut(key)).toBeUndefined();
    }
  });
});
