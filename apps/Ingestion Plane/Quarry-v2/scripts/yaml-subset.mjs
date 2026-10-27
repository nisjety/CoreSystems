// Strict YAML subset parser for the docs-contract whole-file check (scripts/tests/docs-contract.test.mjs).
// It accepts block mappings and sequences (including the compact "key:" then "- item" form), plain, single-
// and double-quoted scalars (single-line or folded over more-indented lines), one-line flow sequences, empty
// flow mappings, literal and folded block scalars, comments and a leading "---". Anything else is an error:
// the check fails closed. A duplicate key in one mapping is an error, which is the failure it exists to catch.
// Checked against yaml.v3 on the tracked YAML files: the same verdict and key structure wherever both accept.

export function parseYamlSubset(text) {
  if (text.includes('\t')) {
    const at = text.split('\n').findIndex((line) => line.includes('\t'));
    throw new Error(`line ${at + 1}: tab character`);
  }
  const raw = text.replace(/\r\n/g, '\n').split('\n');
  const lines = raw.map((content, index) => ({ no: index + 1, content }));
  let pos = 0;

  const isBlank = (line) => line.content.trim() === '' || line.content.trim().startsWith('#');
  const indentOf = (line) => line.content.length - line.content.trimStart().length;
  const textOf = (line) => line.content.trim();
  const skip = () => {
    while (pos < lines.length && isBlank(lines[pos])) pos += 1;
  };

  // The index of a comment that starts outside quotes at `from`, or -1.
  function commentAt(s, from) {
    let quote = null;
    for (let i = from; i < s.length; i += 1) {
      const ch = s[i];
      if (quote === "'") {
        if (ch === "'") {
          if (s[i + 1] === "'") i += 1;
          else quote = null;
        }
      } else if (quote === '"') {
        if (ch === '\\') i += 1;
        else if (ch === '"') quote = null;
      } else if (ch === '#' && (i === from || s[i - 1] === ' ' || s[i - 1] === '\t')) {
        return i;
      } else if ((ch === "'" || ch === '"') && (i === from || /[\s[{,:]/.test(s[i - 1]))) {
        quote = ch;
      }
    }
    return -1;
  }
  const stripComment = (s) => {
    const at = commentAt(s, 0);
    return (at >= 0 ? s.slice(0, at) : s).trim();
  };

  // A quoted scalar at the start of s: returns [value, rest], or null when the quote is not closed in s.
  function quoted(s) {
    const q = s[0];
    if (q === "'") {
      let out = '';
      for (let i = 1; i < s.length; i += 1) {
        if (s[i] === "'") {
          if (s[i + 1] === "'") {
            out += "'";
            i += 1;
          } else return [out, s.slice(i + 1)];
        } else out += s[i];
      }
      return null;
    }
    let out = '';
    for (let i = 1; i < s.length; i += 1) {
      if (s[i] === '\\') {
        out += s[i + 1] ?? '';
        i += 1;
      } else if (s[i] === '"') return [out, s.slice(i + 1)];
      else out += s[i];
    }
    return null;
  }

  // A quoted scalar that is closed on its own line (the rest of the line is empty or a comment).
  function quotedOneLine(s, no) {
    const found = quoted(s);
    if (!found) throw new Error(`line ${no}: quoted scalar does not end on its line`);
    const [value, rest] = found;
    if (rest.trim() !== '' && !rest.trim().startsWith('#')) throw new Error(`line ${no}: text after a quoted scalar`);
    return { t: 's', v: value };
  }

  // A quoted scalar that may run over more-indented lines (a folded flow scalar in block context).
  function quotedAcross(value, indent, no) {
    let buf = value;
    for (;;) {
      const found = quoted(buf);
      if (found) {
        const rest = found[1].trim();
        if (rest !== '' && !rest.startsWith('#')) throw new Error(`line ${no}: text after a quoted scalar`);
        return { t: 's', v: found[0] };
      }
      if (pos >= lines.length) throw new Error(`line ${no}: quoted scalar is not closed`);
      const next = lines[pos];
      if (next.content.trim() !== '' && indentOf(next) <= indent) {
        throw new Error(`line ${next.no}: quoted scalar continues at an outer indentation`);
      }
      buf = `${buf}\n${next.content.trim()}`;
      pos += 1;
    }
  }

  // A scalar that is the whole value text (no key, no block indicator).
  function scalarValue(s, no) {
    if (s === '') return null;
    if (s[0] === "'" || s[0] === '"') return quotedOneLine(s, no);
    if (s[0] === '[') return flowSequence(s, no);
    if (s[0] === '{') {
      if (s.trim() !== '{}') throw new Error(`line ${no}: only empty flow mappings are supported`);
      return { t: 'm', keys: [] };
    }
    if (/^[&*!%@`|>?]/.test(s) || s[0] === '#' || s[0] === ',') {
      throw new Error(`line ${no}: unsupported scalar start: ${s.slice(0, 12)}`);
    }
    if (/: |:$/.test(s)) throw new Error(`line ${no}: mapping values are not allowed in this context`);
    return { t: 's', v: s, plain: true };
  }

  function flowSequence(s, no) {
    if (!s.endsWith(']')) throw new Error(`line ${no}: flow sequence does not end on its line`);
    const inner = s.slice(1, -1).trim();
    const items = [];
    if (inner !== '') {
      for (const part of splitFlow(inner, no)) {
        const item = part.trim();
        if (item === '' || item.startsWith('[') || item.startsWith('{')) {
          throw new Error(`line ${no}: unsupported flow item`);
        }
        items.push(scalarValue(item, no));
      }
    }
    return { t: 'l', items };
  }

  // Splits the inside of a one-line flow sequence at the commas that are outside quotes.
  function splitFlow(inner, no) {
    const parts = [];
    let cur = '';
    let quote = null;
    for (let i = 0; i < inner.length; i += 1) {
      const ch = inner[i];
      if (quote === "'") {
        cur += ch;
        if (ch === "'") {
          if (inner[i + 1] === "'") {
            cur += "'";
            i += 1;
          } else quote = null;
        }
      } else if (quote === '"') {
        cur += ch;
        if (ch === '\\') {
          cur += inner[i + 1] ?? '';
          i += 1;
        } else if (ch === '"') quote = null;
      } else if (ch === ',') {
        parts.push(cur);
        cur = '';
      } else {
        if ((ch === "'" || ch === '"') && cur.trim() === '') quote = ch;
        cur += ch;
      }
    }
    if (quote) throw new Error(`line ${no}: unterminated quote in a flow sequence`);
    parts.push(cur);
    return parts;
  }

  // A key at the start of s: returns [key, rest-after-colon] or null when s is not a mapping entry.
  function splitKey(s, no) {
    if (s[0] === "'" || s[0] === '"') {
      const found = quoted(s);
      if (!found) return null;
      const [key, rest] = found;
      const after = rest.replace(/^\s*/, '');
      if (after[0] !== ':' || (after.length > 1 && after[1] !== ' ')) return null;
      return [key, after.slice(1).trim()];
    }
    if (/^[#[{|>&*!?%@`,\-]/.test(s) && !/^-[^ ]/.test(s)) return null;
    const idx = s.search(/:( |$)/);
    if (idx <= 0) return null;
    return [s.slice(0, idx).trim(), s.slice(idx + 1).trim()];
  }

  // Parses the block that starts at the current line, whose indentation is `indent`.
  function block(indent) {
    skip();
    if (pos >= lines.length) return null;
    const line = lines[pos];
    const ind = indentOf(line);
    if (ind !== indent) throw new Error(`line ${line.no}: unexpected indentation ${ind}, want ${indent}`);
    const t = textOf(line);
    if (t === '-' || t.startsWith('- ')) return sequence(indent);
    if (splitKey(t, line.no)) return mapping(indent);
    pos += 1;
    const value = scalarValue(stripComment(t), line.no);
    return value;
  }

  function mapping(indent) {
    const node = { t: 'm', keys: [], values: new Map() };
    for (;;) {
      skip();
      if (pos >= lines.length) break;
      const line = lines[pos];
      const ind = indentOf(line);
      if (ind < indent) break;
      if (ind > indent) throw new Error(`line ${line.no}: unexpected indentation ${ind}, want ${indent}`);
      const t = textOf(line);
      if (t === '-' || t.startsWith('- ')) break;
      const entry = splitKey(t, line.no);
      if (!entry) throw new Error(`line ${line.no}: expected a mapping key`);
      const [key, rest] = entry;
      if (node.values.has(key)) {
        throw new Error(`line ${line.no}: duplicate key ${JSON.stringify(key)}`);
      }
      node.keys.push(key);
      node.values.set(key, null);
      pos += 1;
      const value = valueAfterKey(indent, rest, line.no);
      node.values.set(key, value);
    }
    return node;
  }

  function valueAfterKey(indent, rest, no) {
    if (rest.startsWith('|') || rest.startsWith('>')) return blockScalar(indent, rest, no);
    const value = stripComment(rest);
    if (value !== '') {
      if (value[0] === "'" || value[0] === '"') {
        if (quoted(value)) return scalarValue(value, no);
        return quotedAcross(value, indent, no);
      }
      if (value[0] === '[' || value[0] === '{') return scalarValue(value, no);
      // A plain scalar may continue on more-indented lines that hold no key.
      let text = value;
      while (pos < lines.length) {
        const next = lines[pos];
        if (isBlank(next)) {
          // a blank line inside a plain scalar is a line break; keep going only if the next text continues
          let look = pos;
          while (look < lines.length && isBlank(lines[look])) look += 1;
          if (look >= lines.length || indentOf(lines[look]) <= indent) break;
          pos = look;
          continue;
        }
        if (indentOf(next) <= indent) break;
        const t = textOf(next);
        if (/: |:$/.test(t)) throw new Error(`line ${next.no}: mapping values are not allowed in this context`);
        text = `${text} ${stripComment(t)}`;
        pos += 1;
      }
      return scalarValue(text, no);
    }
    // Empty value: a nested block, a compact sequence at the same indent, or null.
    skip();
    if (pos >= lines.length) return null;
    const next = lines[pos];
    const ind = indentOf(next);
    const t = textOf(next);
    if (ind > indent) return block(ind);
    if (ind === indent && (t === '-' || t.startsWith('- '))) return sequence(indent);
    return null;
  }

  function blockScalar(indent, header, no) {
    if (!/^[|>][+-]?$/.test(header.trim())) throw new Error(`line ${no}: unsupported block scalar header ${header}`);
    const body = [];
    while (pos < lines.length) {
      const line = lines[pos];
      if (line.content.trim() === '') {
        body.push('');
        pos += 1;
        continue;
      }
      if (indentOf(line) <= indent) break;
      body.push(line.content);
      pos += 1;
    }
    return { t: 's', v: body.join('\n') };
  }

  function sequence(indent) {
    const node = { t: 'l', items: [] };
    for (;;) {
      skip();
      if (pos >= lines.length) break;
      const line = lines[pos];
      const ind = indentOf(line);
      if (ind < indent) break;
      const t = textOf(line);
      if (ind !== indent || !(t === '-' || t.startsWith('- '))) {
        if (ind === indent) break;
        throw new Error(`line ${line.no}: unexpected indentation ${ind} in a sequence at ${indent}`);
      }
      const after = line.content.slice(indent + 1);
      const spaces = after.length - after.trimStart().length;
      const itemText = after.trimStart();
      if (itemText === '') {
        pos += 1;
        skip();
        if (pos < lines.length && indentOf(lines[pos]) > indent) node.items.push(block(indentOf(lines[pos])));
        else node.items.push(null);
        continue;
      }
      const col = indent + 1 + spaces;
      if (itemText.startsWith('|') || itemText.startsWith('>')) {
        pos += 1;
        node.items.push(blockScalar(indent, itemText, line.no));
        continue;
      }
      // Re-read the item as a line at its own column, so a "- key: value" item is a mapping at col.
      lines[pos] = { no: line.no, content: ' '.repeat(col) + itemText };
      const item = block(col);
      node.items.push(item);
    }
    return node;
  }

  skip();
  if (pos < lines.length && textOf(lines[pos]) === '---') {
    pos += 1;
  }
  const root = block(0);
  skip();
  if (pos < lines.length) {
    throw new Error(`line ${lines[pos].no}: content after the document end`);
  }
  return root;
}
