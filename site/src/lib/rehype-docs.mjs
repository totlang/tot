/**
 * The markdown pipeline for README.md and SPEC.md.
 *
 * Those two files are the docs. Rendering them rather than restating them is the only way the
 * site and the language cannot drift apart, so everything here exists to make repository
 * markdown read as a web page: code blocks get the site's colours, and the relative links that
 * work on GitHub get pointed somewhere that works here.
 */

import { createHighlighter } from 'shiki';
import { visit } from 'unist-util-visit';

import { toHast } from './tot-highlight.mjs';

const REPO = 'https://github.com/totlang/tot';
const BLOB = `${REPO}/blob/main/`;
const TREE = `${REPO}/tree/main/`;

// tot is highlighted by its own tokeniser; everything else is Shiki. Keeping the list explicit
// means an unknown fence renders as plain text instead of failing the build.
const OWN = new Set(['tot', 'tott']);
const SHIKI = ['bash', 'rust', 'toml', 'json', 'yaml'];

/**
 * The site palette as a TextMate theme, so a Rust snippet and a tot snippet look like they
 * belong on the same page. Code blocks are dark in both site themes, which is what lets one
 * theme cover both.
 */
export const THEME = {
  name: 'tot-crust',
  type: 'dark',
  colors: { 'editor.background': '#1B1409', 'editor.foreground': '#F6EBD8' },
  settings: [
    { settings: { background: '#1B1409', foreground: '#F6EBD8' } },
    {
      scope: ['comment', 'punctuation.definition.comment'],
      settings: { foreground: '#9C8563', fontStyle: 'italic' },
    },
    { scope: ['string', 'string.quoted', 'meta.string'], settings: { foreground: '#F2B950' } },
    { scope: ['constant.numeric', 'constant.other'], settings: { foreground: '#E8955A' } },
    {
      scope: ['constant.language', 'constant.character.escape', 'variable.language'],
      settings: { foreground: '#D9C58E' },
    },
    {
      scope: ['keyword', 'keyword.control', 'storage', 'storage.type', 'storage.modifier'],
      settings: { foreground: '#F7C86A' },
    },
    {
      scope: ['entity.name.function', 'support.function', 'meta.function-call', 'entity.name.tag'],
      settings: { foreground: '#E0A038' },
    },
    {
      scope: ['entity.name.type', 'support.type', 'entity.name.namespace', 'support.class'],
      settings: { foreground: '#D9C58E' },
    },
    { scope: ['variable', 'variable.other', 'meta.definition'], settings: { foreground: '#F6EBD8' } },
    {
      scope: ['punctuation', 'meta.brace', 'keyword.operator'],
      settings: { foreground: '#8A6F45' },
    },
  ],
};

let pending;
function highlighter() {
  pending ??= createHighlighter({ themes: [THEME], langs: SHIKI });
  return pending;
}

function textOf(node) {
  if (node.type === 'text') return node.value;
  if (!node.children) return '';
  return node.children.map(textOf).join('');
}

function element(tagName, className, children) {
  return {
    type: 'element',
    tagName,
    properties: className ? { className: [className] } : {},
    children,
  };
}

/** Wraps highlighted tokens in the site's code chrome. The copy button is added client-side. */
function codeBlock(lang, children) {
  return {
    type: 'element',
    tagName: 'figure',
    properties: { className: ['codeblock'], 'data-lang': lang || 'text' },
    children: [
      element('div', 'codeblock-bar', [
        element('span', 'codeblock-lang', [{ type: 'text', value: lang || 'text' }]),
      ]),
      element('pre', null, [element('code', null, children)]),
    ],
  };
}

async function render(lang, code) {
  if (OWN.has(lang)) return toHast(code, lang);

  if (SHIKI.includes(lang)) {
    const shiki = await highlighter();
    const root = shiki.codeToHast(code, { lang, theme: THEME.name });
    // Shiki hands back <pre><code>…</code></pre>; only the token spans are wanted, since the
    // chrome around them is this site's.
    const pre = root.children.find((c) => c.type === 'element' && c.tagName === 'pre');
    const inner = pre?.children.find((c) => c.type === 'element' && c.tagName === 'code');
    if (inner) return inner.children;
  }

  return [{ type: 'text', value: code }];
}

/** Replaces every fenced block with the site's highlighted equivalent. */
export function rehypeCode() {
  return async (tree) => {
    const jobs = [];

    visit(tree, 'element', (node, index, parent) => {
      if (node.tagName !== 'pre' || !parent || index === null || index === undefined) return;
      const code = node.children.find((c) => c.type === 'element' && c.tagName === 'code');
      if (!code) return;

      const classes = code.properties?.className ?? [];
      const marker = (Array.isArray(classes) ? classes : []).find(
        (c) => typeof c === 'string' && c.startsWith('language-'),
      );

      jobs.push({
        parent,
        index,
        lang: marker ? marker.slice('language-'.length) : '',
        // Markdown always leaves a trailing newline on a fence; keeping it would draw an empty
        // final line in every block.
        code: textOf(code).replace(/\n$/, ''),
      });
    });

    for (const job of jobs) {
      job.parent.children[job.index] = codeBlock(job.lang, await render(job.lang, job.code));
    }
  };
}

/**
 * Repoints the links that were written for a repository.
 *
 * `SPEC.md` and `README.md` have pages here, anchors and all — the two files are slugged by the
 * same code, so a cross-file anchor survives. Everything else relative is a file that only
 * exists in the repository, so it goes to the repository.
 */
export function rehypeLinks() {
  return (tree) => {
    visit(tree, 'element', (node) => {
      if (node.tagName !== 'a') return;
      const href = node.properties?.href;
      if (typeof href !== 'string' || href === '') return;

      if (href.startsWith('#')) return;

      if (/^[a-z][a-z0-9+.-]*:/i.test(href)) {
        if (/^https?:/i.test(href)) {
          node.properties.target = '_blank';
          node.properties.rel = 'noopener noreferrer';
        }
        return;
      }

      const [path, fragment] = href.split('#');
      const anchor = fragment ? `#${fragment}` : '';

      if (path === 'SPEC.md') {
        node.properties.href = `/spec${anchor}`;
        return;
      }
      if (path === 'README.md' || path === '') {
        node.properties.href = `/docs${anchor}`;
        return;
      }

      // GitHub serves a directory under /tree/ and a file under /blob/; a directory reached
      // through /blob/ is a 404, so the trailing slash has to decide which.
      const base = path.endsWith('/') ? TREE : BLOB;
      node.properties.href = base + path.replace(/\/$/, '') + anchor;
      node.properties.target = '_blank';
      node.properties.rel = 'noopener noreferrer';
    });
  };
}
