# site — totlang.dev

The tot website. Astro, statically generated, served as a directory of files.

```bash
cd site
pnpm install
pnpm dev               # http://localhost:4321
pnpm build             # builds the wasm, then the site, into dist/
```

pnpm is the package manager here, declared in `package.json` so corepack picks it up; there is
no `package-lock.json` and running `npm install` would create one that drifts from
`pnpm-lock.yaml`.

`pnpm build` runs `pnpm run wasm` first, so a build needs **Rust 1.88+, the
`wasm32-unknown-unknown` target, and `wasm-pack`** as well as Node. `pnpm build:site` skips
that step when `src/wasm/` is already up to date.

Both pass `--force`, which clears the content layer cache, and that is not optional: Astro keeps
the *rendered* HTML for `README.md` and `SPEC.md` in `node_modules/.astro/data-store.json`, and
editing `src/lib/rehype-docs.mjs` does not invalidate it. Without `--force` a change to the
markdown pipeline builds cleanly and ships the previous output.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

## What is where

| | |
|---|---|
| `src/pages/index.astro` | the landing page — the only hand-written copy on the site |
| `src/pages/docs.astro` | renders `../README.md` |
| `src/pages/spec.astro` | renders `../SPEC.md` |
| `src/pages/play.astro` | the playground shell; the logic is `src/lib/playground.ts` |
| `src/lib/tot-highlight.mjs` | the tot syntax highlighter, used at build time and in the browser |
| `src/lib/rehype-docs.mjs` | the markdown pipeline: code blocks and link rewriting |
| `src/styles/tokens.css` | the design system |
| `wasm/` | `tot-wasm`, the playground's bridge into the library |

## Three things worth knowing before editing

**The docs are the repository's own markdown.** `/docs` is `README.md` and `/spec` is `SPEC.md`,
loaded through a content-layer loader with its base outside this directory. Nothing about the
language is restated here, so the site cannot fall behind it. Editing the docs means editing
those two files. (A Vite import will not work — Astro only runs markdown inside `srcDir` through
its pipeline — which is why it is a loader.)

Relative links in those files are rewritten on the way out: `SPEC.md` becomes `/spec`, and
anything else relative becomes a link into the repository on GitHub.

**The playground runs the real parser.** `wasm/` compiles the `tot` crate to WebAssembly, so
every answer the page gives is the answer `tot` would give, including the caret diagnostics —
they come from the same `render` call the CLI uses. The YAML and TOML converters are the CLI's
`convert.rs`, included by path rather than copied, so the two cannot disagree.

The wasm crate is **excluded from the cargo workspace** (see the root `Cargo.toml`), which keeps
`wasm-bindgen` and the converters' dependencies out of `cargo test --workspace`. The library
having no dependencies is the point; the site should not be what changes that.

The playground's samples are `../examples/*`, read at build time. A playground that drifted from
the files the tests check would be worse than no playground.

**Code blocks are dark in both themes.** The page has a light theme and a dark one; code does
not. That is what lets one set of token colours be correct everywhere, and it makes code read as
a distinct material rather than as tinted prose. The token colours are in `tokens.css` and the
Shiki theme built from them is in `rehype-docs.mjs` — change both together.

## Deploying

`pnpm build` produces `dist/`, a directory of static files with no server-side part. How
that directory reaches a server is not this repository's business: the runbook, the web server
configuration and the DNS live with the infrastructure that owns them.

Two things about the output are worth knowing wherever it ends up:

- **Clean URLs need a directory-style fallback.** Pages build to `docs/index.html`, not
  `docs.html`, so `/docs` has to resolve through `{path}/index.html`. This is a static tree,
  not a single-page app: a request for a path that has no page should still 404, and the built
  `404.html` is the page to serve when it does.
- **The playground streams WebAssembly.** `_astro/*.wasm` must be served as `application/wasm`
  or the browser refuses to compile it and only that one page silently stops working.

Everything under `_astro/` is content-hashed and safe to cache forever. Everything else — the
HTML, `favicon.svg`, `robots.txt`, the sitemap — should revalidate, so a redeploy is picked up
without waiting out a cache.
