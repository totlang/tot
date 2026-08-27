import { glob } from 'astro/loaders';
import { defineCollection } from 'astro:content';

/**
 * The documentation is the repository's own README.md and SPEC.md.
 *
 * They are loaded from outside the site directory on purpose: rendering the files the language
 * already ships is the only arrangement in which the docs cannot fall behind it. A Vite import
 * would not work — Astro only runs markdown inside `srcDir` through its pipeline — but a loader
 * reads by path, so the two files stay exactly where they belong.
 */
const docs = defineCollection({
  loader: glob({ base: '../', pattern: ['README.md', 'SPEC.md'] }),
});

export const collections = { docs };
