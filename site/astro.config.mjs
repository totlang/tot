import sitemap from '@astrojs/sitemap';
import { defineConfig } from 'astro/config';

import { rehypeCode, rehypeLinks } from './src/lib/rehype-docs.mjs';

export default defineConfig({
  site: 'https://totlang.dev',
  // public/robots.txt points at /sitemap-index.xml, so one has to exist.
  integrations: [sitemap({ filter: (page) => !page.endsWith('/404/') })],
  // Directory output: /docs is docs/index.html rather than docs.html. Whatever serves this
  // needs a matching directory-index fallback — see "Deploying" in README.md.
  build: { format: 'directory' },
  markdown: {
    // Shiki is driven from rehypeCode instead, so that tot can be highlighted by its own
    // tokeniser and everything else by Shiki, both wrapped in the same chrome.
    syntaxHighlight: false,
    rehypePlugins: [rehypeCode, rehypeLinks],
  },
});
