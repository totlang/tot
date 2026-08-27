/**
 * A syntax highlighter for tot.
 *
 * The language is small enough that this is a tokeniser and a four-state machine rather than a
 * grammar, and small enough to ship to the browser: the playground re-highlights on every
 * keystroke, so the docs build and the editor overlay run the same code rather than one of them
 * pulling in a general-purpose highlighter.
 *
 * The point of the colouring is to teach the one rule that catches everyone: **string values are
 * quoted and keys are not**. Strings are the loudest thing on the page and keys recede, so the
 * distinction is visible before any prose explains it. A bareword sitting where a value belongs
 * is a parse error in tot, so it is coloured as one while you type.
 */

/** @typedef {'tot' | 'tott'} Dialect */
/** @typedef {'ws'|'sep'|'comment'|'string'|'block'|'number'|'literal'|'word'|'open'|'close'|'bad'} Kind */
/** @typedef {{ text: string, kind: Kind, cls?: string, quoted?: boolean }} Token */

// Reserved outside strings, per SPEC.md. `(` and `)` join them in a template and stay ordinary
// bareword characters in a document, which is the only difference between the two dialects.
const RESERVED = new Set([',', ':', '"', '{', '}', '[', ']', '#', '=']);

// `1.` and `.1` are legal and normalise to `1.0` and `0.1`.
const NUMBER = /^-?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/;

// A leading zero is an error rather than a number that quietly lost a digit, which is why zip
// codes have to be strings. Worth colouring, since the whole point is that it does not go
// unnoticed.
const LEADING_ZERO = /^-?0\d/;

/**
 * Whether a number lexeme is one tot will actually accept. An integer keeps its lexeme and has
 * no range limit, so only a float can be out of range — and tot has no way to write an infinity,
 * which makes `1e999` a parse error while `1e-999` is a perfectly ordinary zero.
 */
function isWritable(text) {
  if (LEADING_ZERO.test(text)) return false;
  const float = /[.eE]/.test(text);
  return !float || Number.isFinite(Number.parseFloat(text));
}

const isSpace = (c) => c === ' ' || c === '\t' || c === '\r' || c === '\n';

function isReserved(c, dialect) {
  if (isSpace(c) || c < ' ') return true;
  if (RESERVED.has(c)) return true;
  return dialect === 'tott' && (c === '(' || c === ')');
}

/**
 * Splits a source into tokens. Never throws: a half-typed string or an unclosed brace is an
 * ordinary state in an editor, so the lexer runs to the end of the input whatever it finds.
 *
 * @param {string} src
 * @param {Dialect} dialect
 * @returns {Token[]}
 */
export function lex(src, dialect = 'tot') {
  /** @type {Token[]} */
  const out = [];
  let i = 0;

  while (i < src.length) {
    const c = src[i];

    if (isSpace(c)) {
      let j = i;
      while (j < src.length && isSpace(src[j])) j++;
      out.push({ text: src.slice(i, j), kind: 'ws' });
      i = j;
      continue;
    }

    // `,` and `:` are whitespace to the parser. They are still drawn, dimmed, because a reader
    // pasting JSON should be able to see that they were kept and that they mean nothing.
    if (c === ',' || c === ':') {
      out.push({ text: c, kind: 'sep' });
      i++;
      continue;
    }

    if (c === '#') {
      let j = i;
      while (j < src.length && src[j] !== '\n') j++;
      out.push({ text: src.slice(i, j), kind: 'comment' });
      i = j;
      continue;
    }

    if (src.startsWith('"""', i)) {
      let j = i + 3;
      while (j < src.length && !src.startsWith('"""', j)) j++;
      j = Math.min(src.length, j + 3);
      out.push({ text: src.slice(i, j), kind: 'block' });
      i = j;
      continue;
    }

    if (c === '"') {
      let j = i + 1;
      while (j < src.length) {
        if (src[j] === '\\') {
          j += 2;
          continue;
        }
        if (src[j] === '"') {
          j++;
          break;
        }
        // A quoted string cannot span a line, so stopping here keeps one missing quote from
        // painting the rest of the file gold.
        if (src[j] === '\n') break;
        j++;
      }
      out.push({ text: src.slice(i, j), kind: 'string' });
      i = j;
      continue;
    }

    if (c === '{' || c === '[' || (dialect === 'tott' && c === '(')) {
      out.push({ text: c, kind: 'open' });
      i++;
      continue;
    }

    if (c === '}' || c === ']' || (dialect === 'tott' && c === ')')) {
      out.push({ text: c, kind: 'close' });
      i++;
      continue;
    }

    if (c === '=') {
      out.push({ text: c, kind: 'bad' });
      i++;
      continue;
    }

    let j = i;
    while (j < src.length && !isReserved(src[j], dialect)) j++;
    const text = src.slice(i, j);
    const kind = NUMBER.test(text)
      ? 'number'
      : text === 'true' || text === 'false' || text === 'null'
        ? 'literal'
        : 'word';
    out.push({ text, kind });
    i = j;
  }

  return out;
}

/**
 * Decides what each token *is*, which needs context: the same bareword is a key at the start of
 * a member and a parse error one token later. Objects alternate key and value, arrays are all
 * values, and a form's first token is its head.
 *
 * @param {Token[]} tokens
 * @returns {Token[]}
 */
export function classify(tokens) {
  /** @type {{kind: 'object'|'array'|'form', expect: 'key'|'value', first?: boolean}[]} */
  const stack = [{ kind: 'object', expect: 'key' }];
  const top = () => stack[stack.length - 1];

  // A collection occupies one value slot, so the enclosing object goes back to expecting a key
  // only once the collection closes.
  const valueDone = () => {
    const frame = top();
    if (frame.kind === 'object') frame.expect = 'key';
  };

  for (const token of tokens) {
    if (token.kind === 'ws') continue;

    if (token.kind === 'comment') {
      token.cls = 'c';
      continue;
    }
    if (token.kind === 'sep') {
      token.cls = 'p';
      continue;
    }
    if (token.kind === 'bad') {
      token.cls = 'e';
      continue;
    }

    if (token.kind === 'open') {
      token.cls = 'p';
      if (token.text === '{') stack.push({ kind: 'object', expect: 'key' });
      else if (token.text === '[') stack.push({ kind: 'array', expect: 'value' });
      else stack.push({ kind: 'form', expect: 'value', first: true });
      continue;
    }

    if (token.kind === 'close') {
      token.cls = 'p';
      // Never pop the document itself; a stray `}` while typing should not make everything
      // after it a key.
      if (stack.length > 1) stack.pop();
      valueDone();
      continue;
    }

    const frame = top();

    if (frame.kind === 'form' && frame.first) {
      token.cls = 'f';
      frame.first = false;
      continue;
    }

    if (frame.kind === 'object' && frame.expect === 'key') {
      token.cls = 'k';
      token.quoted = token.kind === 'string';
      frame.expect = 'value';
      continue;
    }

    token.cls =
      token.kind === 'string' || token.kind === 'block'
        ? 's'
        : token.kind === 'number'
          ? isWritable(token.text)
            ? 'n'
            : 'e'
          : token.kind === 'literal'
            ? 'b'
            : // A bareword in value position is the mistake the language exists to make loud.
              'e';
    valueDone();
  }

  return tokens;
}

/** @param {string} src @param {Dialect} dialect */
export function tokens(src, dialect = 'tot') {
  return classify(lex(src, dialect));
}

const escapeHtml = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/**
 * Highlights a source into an HTML string. Used for build-time samples and for the playground's
 * editor overlay, which is why it emits nothing but spans and the original whitespace: the
 * overlay has to line up with a textarea character for character.
 *
 * @param {string} src
 * @param {Dialect} dialect
 * @returns {string}
 */
export function highlight(src, dialect = 'tot') {
  let out = '';
  for (const token of tokens(src, dialect)) {
    if (!token.cls) {
      out += escapeHtml(token.text);
      continue;
    }
    // A quoted key keeps its quotes dim so the name still reads as structure rather than as one
    // more string. This is what JSON pasted into the playground looks like.
    if (token.quoted && token.text.length >= 2 && token.text.endsWith('"')) {
      out +=
        '<span class="tp">"</span>' +
        `<span class="tk">${escapeHtml(token.text.slice(1, -1))}</span>` +
        '<span class="tp">"</span>';
      continue;
    }
    out += `<span class="t${token.cls}">${escapeHtml(token.text)}</span>`;
  }
  return out;
}

/**
 * The same highlighting as hast nodes, for the rehype plugin that renders README.md and SPEC.md.
 * Building nodes rather than a raw HTML string keeps the markdown pipeline free of raw passes.
 *
 * @param {string} src
 * @param {Dialect} dialect
 */
export function toHast(src, dialect = 'tot') {
  const span = (cls, value) => ({
    type: 'element',
    tagName: 'span',
    properties: { className: [cls] },
    children: [{ type: 'text', value }],
  });

  const children = [];
  for (const token of tokens(src, dialect)) {
    if (!token.cls) {
      children.push({ type: 'text', value: token.text });
      continue;
    }
    if (token.quoted && token.text.length >= 2 && token.text.endsWith('"')) {
      children.push(span('tp', '"'), span('tk', token.text.slice(1, -1)), span('tp', '"'));
      continue;
    }
    children.push(span(`t${token.cls}`, token.text));
  }
  return children;
}
