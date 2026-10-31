#!/usr/bin/env python3
"""Log-content gate: no Model Plane log line may carry turn content.

Build plan WS12 S10. A log line that carries a prompt, completion, transcript
or request or response body persists that content outside the zero-retention
boundary, so every `trace!`, `debug!`, `info!`, `warn!`, `error!` and `event!`
invocation in the four Model Plane Rust services is checked. The check is
syntactic, not taint analysis. The source is tokenised (strings and comments
are skipped as code; format strings are read only for inline captures), and a
macro is flagged when its arguments name:

- a content name (prompt, system_prompt, completion, transcript, user_message,
  answer, snippet, content, text, body) anywhere: as a field, a value or an
  inline capture such as `{text}`; or
- a body-like name (req, request, body, messages) whose value is formatted with
  a `%` or `?` sigil, or passed to a `{:?}` placeholder. Only the last segment
  of a field chain counts, so `%req.org_id` is an identifier, not the request.

Name matches are false positives too (`resp.text()`, `messages.len()`). A
reviewer accepts a site in one of two ways: the baseline, or an allow comment on
the macro's first line, on the line above it, or inside it:

    // log-content: allow(<why this is not turn content>)

Baseline entries are keyed by the file path and the sha256 of each finding's
normalised macro text (comments dropped, whitespace collapsed). Line numbers and
file hashes are deliberately not keys, so edits elsewhere in a file do not churn
the shared baseline. Every entry carries a `review`, the reviewer's reason the
site is not turn content. A finding the baseline lacks, or holds without a
review, fails the gate. A baseline entry that no longer occurs is reported as
stale and passes. `--update` rewrites the baseline and keeps the reviews of the
sites it still finds.

Usage, from the repository root:
    python3 scripts/gates/log-content.py --check
    python3 scripts/gates/log-content.py --update
Exit codes: 0 ok, 1 a finding the baseline lacks or holds without a review,
2 the gate cannot run (missing scope directory or baseline, unbalanced macro
invocation).
"""
import argparse
import hashlib
import json
import re
import sys
from collections import Counter
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BASELINE = REPO_ROOT / 'scripts' / 'gates' / 'baselines' / 'log-content.json'
SCOPE = (
    'apps/Model Plane/rust/services/model-gateway/src',
    'apps/Model Plane/rust/services/execution-core/src',
    'apps/Model Plane/rust/services/inference-core/src',
    'apps/Model Plane/rust/services/session-core/src',
)
LOG_MACROS = frozenset({'trace', 'debug', 'info', 'warn', 'error', 'event'})
CONTENT_NAMES = frozenset({
    'prompt', 'system_prompt', 'completion', 'transcript', 'user_message',
    'answer', 'snippet', 'content', 'text', 'body',
})
DEBUG_NAMES = frozenset({'req', 'request', 'body', 'messages'})
OPENERS = {'(': ')', '[': ']', '{': '}'}
CLOSERS = frozenset(OPENERS.values())
# Sigils sit at the start of an argument, after the opening delimiter, a comma
# or the `=` of a field.
SIGIL_PREDECESSORS = frozenset({'(', '[', '{', ',', '='})
TWO_CHAR_PUNCT = ('::', '=>', '==', '!=', '->', '..', '&&', '||', '<=', '>=')

IDENT_RE = re.compile(r'[^\W\d]\w*')
NUMBER_RE = re.compile(r'\d\w*(?:\.\d\w*)*')
RAW_STRING_RE = re.compile(r'[bc]?r(#*)"')
# Unrolled so that a long string literal does not backtrack per character.
STRING_BODY_RE = re.compile(r'[^"\\]*(?:\\.[^"\\]*)*"', re.DOTALL)
PLACEHOLDER_RE = re.compile(r'\{\{|\}\}|\{([^{}]*)\}')
ALLOW_RE = re.compile(r'log-content:\s*allow\(([^()]*[^()\s][^()]*)\)')


class GateError(Exception):
    """The gate cannot make a decision; this is not a finding."""


def _block_comment_end(src, i):
    depth, j = 0, i
    while j < len(src):
        if src.startswith('/*', j):
            depth += 1
            j += 2
        elif src.startswith('*/', j):
            depth -= 1
            j += 2
            if depth == 0:
                return j
        else:
            j += 1
    return len(src)


def _char_or_lifetime(src, q):
    """Return (kind, end) for the quote at index q: a char literal, or a lifetime or label."""
    if src.startswith('\\', q + 1):
        j = src.find("'", q + 3)
        return 'chr', (len(src) if j < 0 else j + 1)
    if src.startswith("'", q + 2):
        return 'chr', q + 3
    m = IDENT_RE.match(src, q + 1)
    return 'life', (m.end() if m else q + 1)


def lex(src):
    """Tokenise Rust source into (kind, text, line) tuples; comments are kept."""
    toks = []
    i, n, line = 0, len(src), 1
    while i < n:
        c = src[i]
        if c.isspace():
            if c == '\n':
                line += 1
            i += 1
            continue
        raw = RAW_STRING_RE.match(src, i)
        ident = IDENT_RE.match(src, i)
        if src.startswith('//', i):
            j = src.find('\n', i)
            kind, j = 'comment', (n if j < 0 else j)
        elif src.startswith('/*', i):
            kind, j = 'comment', _block_comment_end(src, i)
        elif raw:
            close = '"' + raw.group(1)
            j = src.find(close, raw.end())
            kind, j = 'str', (n if j < 0 else j + len(close))
        elif c == '"' or (c in 'bc' and src.startswith('"', i + 1)):
            m = STRING_BODY_RE.match(src, i + 1 if c == '"' else i + 2)
            kind, j = 'str', (n if m is None else m.end())
        elif c == "'" or (c == 'b' and src.startswith("'", i + 1)):
            kind, j = _char_or_lifetime(src, i if c == "'" else i + 1)
        elif ident:
            kind, j = 'id', ident.end()
        elif NUMBER_RE.match(src, i):
            kind, j = 'num', NUMBER_RE.match(src, i).end()
        else:
            op = next((p for p in TWO_CHAR_PUNCT if src.startswith(p, i)), c)
            kind, j = 'punct', i + len(op)
        toks.append((kind, src[i:j], line))
        line += src.count('\n', i, j)
        i = j
    return toks


def _is_punct(tok, text):
    return tok is not None and tok[0] == 'punct' and tok[1] == text


def _match_close(toks, open_idx):
    """Index of the delimiter that closes toks[open_idx], or None when the brackets do not nest."""
    expected = []
    for k in range(open_idx, len(toks)):
        kind, text, _ = toks[k]
        if kind != 'punct':
            continue
        if text in OPENERS:
            expected.append(OPENERS[text])
        elif text in CLOSERS:
            if not expected or expected.pop() != text:
                return None
            if not expected:
                return k
    return None


def _log_macros(toks):
    """Yield (name_idx, open_idx, close_idx) for each log macro; close_idx is None when unbalanced."""
    for k in range(len(toks) - 2):
        kind, name, _ = toks[k]
        if (kind == 'id' and name in LOG_MACROS and _is_punct(toks[k + 1], '!')
                and toks[k + 2][0] == 'punct' and toks[k + 2][1] in OPENERS):
            yield k, k + 2, _match_close(toks, k + 2)


def _split_args(body):
    segments, cur, depth = [], [], 0
    for tok in body:
        if tok[0] == 'punct':
            if tok[1] in OPENERS:
                depth += 1
            elif tok[1] in CLOSERS:
                depth -= 1
            elif tok[1] == ',' and depth == 0:
                segments.append(cur)
                cur = []
                continue
        cur.append(tok)
    segments.append(cur)
    return segments


def _format_string(body):
    """The message literal: the first argument that starts with a string and is not a field."""
    for seg in _split_args(body):
        if seg and seg[0][0] == 'str' and not (len(seg) > 1 and _is_punct(seg[1], '=')):
            return seg[0][1]
    return None


def _placeholders(fmt):
    for m in PLACEHOLDER_RE.finditer(fmt):
        if m.group(1) is not None:
            arg, _, spec = m.group(1).partition(':')
            yield arg.strip(), '?' in spec


def _chain_start(body, idx):
    """Walk back over `a.b.c` so a sigil on the expression is found before its first segment."""
    s = idx
    while s >= 2 and _is_punct(body[s - 1], '.') and body[s - 2][0] == 'id':
        s -= 2
    return s


def _sigil_before(body, s):
    if s < 1 or not (body[s - 1][0] == 'punct' and body[s - 1][1] in ('%', '?')):
        return False
    return s < 2 or (body[s - 2][0] == 'punct' and body[s - 2][1] in SIGIL_PREDECESSORS)


def _next_tok(body, idx):
    return body[idx + 1] if idx + 1 < len(body) else None


def _macro_reasons(body, fmt):
    reasons = []
    positional_debug = False
    if fmt is not None:
        for arg, debug in _placeholders(fmt):
            if arg in CONTENT_NAMES:
                reasons.append(f'capture:{arg}')
            if debug and arg in DEBUG_NAMES:
                reasons.append(f'debug-capture:{arg}')
            elif debug and (arg == '' or arg.isdigit()):
                positional_debug = True
    for idx, (kind, text, _) in enumerate(body):
        if kind != 'id':
            continue
        if text in CONTENT_NAMES:
            reasons.append(f'name:{text}')
        elif text in DEBUG_NAMES:
            # Only the last segment of a field chain is the value (`req.org_id` is not the
            # request), and `request = ...` names a field rather than passing the value.
            nxt = _next_tok(body, idx)
            if _is_punct(nxt, '.') or _is_punct(nxt, '='):
                continue
            if _sigil_before(body, _chain_start(body, idx)):
                reasons.append(f'sigil:{text}')
            elif positional_debug:
                reasons.append(f'debug-arg:{text}')
    return list(dict.fromkeys(reasons))


def scan_file(rel, src):
    """Return (findings, allowed) for one source file; allowed counts flagged sites an allow comment covers."""
    toks = lex(src)
    allow_lines = {line for kind, text, line in toks if kind == 'comment' and ALLOW_RE.search(text)}
    findings, allowed = [], 0
    for k, open_idx, close_idx in _log_macros(toks):
        line = toks[k][2]
        if close_idx is None:
            raise GateError(f'{rel}:{line}: unbalanced {toks[k][1]}! invocation')
        body = [t for t in toks[open_idx + 1:close_idx] if t[0] != 'comment']
        reasons = _macro_reasons(body, _format_string(body))
        if not reasons:
            continue
        inside = any(kind == 'comment' and ALLOW_RE.search(text) for kind, text, _ in toks[open_idx:close_idx])
        if inside or line in allow_lines or line - 1 in allow_lines:
            allowed += 1
            continue
        parts = [t[1] for t in toks[k:close_idx + 1] if t[0] != 'comment']
        if len(parts) > 2 and parts[-2] == ',':
            del parts[-2]  # a trailing comma is layout, not a different log line
        text = ' '.join(parts)
        findings.append({
            'path': rel,
            'line': line,
            'macro': toks[k][1] + '!',
            'reasons': reasons,
            'text': text,
            'sha256': hashlib.sha256(text.encode('utf-8')).hexdigest(),
        })
    return findings, allowed


def scan(root):
    findings, allowed = [], 0
    for rel in SCOPE:
        base = root / rel
        if not base.is_dir():
            raise GateError(f'scope directory missing: {rel}')
        for path in sorted(base.rglob('*.rs')):
            file_findings, file_allowed = scan_file(
                path.relative_to(root).as_posix(), path.read_text(encoding='utf-8'))
            findings.extend(file_findings)
            allowed += file_allowed
    return findings, allowed


def load_baseline(path):
    """Return (occurrence counts, reviews) keyed by (path, sha256)."""
    if not path.is_file():
        raise GateError(f'baseline missing: {path.as_posix()}; create it with --update in a reviewed change')
    data = json.loads(path.read_text(encoding='utf-8'))
    counts, reviews = Counter(), {}
    for e in data['findings']:
        key = (e['path'], e['sha256'])
        counts[key] += 1
        if e.get('review'):
            reviews.setdefault(key, e['review'])
    return counts, reviews


def write_baseline(path, findings, reviews):
    """Rewrite the baseline. A review is the reviewer's reason the site is not turn content; a new site has none."""
    entries = [
        {'path': f['path'], 'macro': f['macro'], 'reasons': f['reasons'], 'sha256': f['sha256'],
         'text': f['text'], 'review': reviews.get((f['path'], f['sha256']), '')}
        for f in sorted(findings, key=lambda f: (f['path'], f['sha256']))
    ]
    doc = {'version': 1, 'scope': list(SCOPE), 'count': len(entries), 'findings': entries}
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=1, sort_keys=True) + '\n', encoding='utf-8', newline='\n')


def _show(text):
    return text[:240].replace('\n', ' ').encode('ascii', 'backslashreplace').decode('ascii')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument('--check', action='store_true', help='fail on findings the baseline lacks or holds unreviewed (default)')
    mode.add_argument('--update', action='store_true', help='rewrite the baseline from the current findings (a reviewed step)')
    ap.add_argument('--root', type=Path, default=REPO_ROOT, help=argparse.SUPPRESS)
    ap.add_argument('--baseline', type=Path, default=BASELINE, help=argparse.SUPPRESS)
    args = ap.parse_args(argv)
    try:
        findings, allowed = scan(args.root.resolve())
        if args.update:
            reviews = load_baseline(args.baseline)[1] if args.baseline.is_file() else {}
            write_baseline(args.baseline, findings, reviews)
            print(f'baseline rewritten: {len(findings)} findings, {allowed} allowed by comment')
            return 0
        counts, reviews = load_baseline(args.baseline)
    except GateError as exc:
        print(f'::error::log-content: {exc}')
        return 2

    current = Counter((f['path'], f['sha256']) for f in findings)
    stale = counts - current
    if stale:
        print(f'note: {sum(stale.values())} baseline entries no longer occur; prune with --update in a reviewed change')
    # A finding is a problem when the baseline lacks it or holds it without a review.
    unseen = current - counts
    problems = []
    for f in findings:
        key = (f['path'], f['sha256'])
        if unseen[key] > 0:
            unseen[key] -= 1
            problems.append((f, 'not in the baseline'))
        elif not reviews.get(key):
            problems.append((f, 'baseline entry has no review'))
    if problems:
        print(f'::error::log-content: {len(problems)} finding(s) need a reviewed baseline entry or a fix')
        for f, why in problems[:50]:
            print(f"  {f['path']}:{f['line']}: {f['macro']} {', '.join(f['reasons'])} ({why})")
            print(f"      {_show(f['text'])}")
        if len(problems) > 50:
            print(f'  ... and {len(problems) - 50} more')
        return 1
    print(f'log-content gate ok: {len(findings)} findings, all reviewed '
          f'({sum(counts.values())} in baseline), {allowed} allowed by comment')
    return 0


if __name__ == '__main__':
    sys.exit(main())
