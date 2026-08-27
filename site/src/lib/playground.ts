/**
 * The playground.
 *
 * Every answer on this page comes from the real parser compiled to WebAssembly, so what it says
 * about a document is what `tot` would say about it — including the diagnostics, which are the
 * same `render` output the CLI prints. Nothing is sent anywhere.
 */

import { highlight } from './tot-highlight.mjs';
import init, {
  build as wasmBuild,
  check_schema as wasmCheckSchema,
  convert as wasmConvert,
  format as wasmFormat,
} from '../wasm/tot.js';

// The samples are the repository's own examples, read at build time. A playground that drifts
// from the files the tests check would be worse than no playground.
import configTot from '../../../examples/config.tot?raw';
import configSchemaTot from '../../../examples/config.schema.tot?raw';
import defaultsTot from '../../../examples/defaults.tot?raw';
import regionsTot from '../../../examples/regions.tot?raw';
import serviceTott from '../../../examples/service.tott?raw';

type Mode = 'convert' | 'schema' | 'template';
type Dialect = 'tot' | 'tott';

interface Param {
  name: string;
  value: string;
  raw: boolean;
}

interface State {
  mode: Mode;
  convert: { source: string; target: string; compact: boolean };
  schema: { document: string; shape: string };
  template: { files: Record<string, string>; entry: string; active: string; params: Param[] };
}

interface Diagnostic {
  render: string;
  line?: number;
  column?: number;
}

type Result =
  | {
      ok: true;
      value?: string;
      warnings?: Diagnostic[];
      /** Things the conversion did, as against things wrong with the document. Counted apart. */
      notes?: Diagnostic[];
      violations?: Diagnostic[];
      imports?: { name: string; bytes: number; reads: number }[];
    }
  | { ok: false; error: string; line?: number; column?: number; where?: string };

// --- samples -------------------------------------------------------------------------------

const JSON_SAMPLE = `{
  "name": "example-service",
  "version": 3,
  "listen": { "host": "0.0.0.0", "port": 8080 },
  "regions": ["us-west-2", "eu-central-1"],
  "retries": null
}`;

// The same document with two things wrong: a float written as a string, and a key with its
// letters swapped. A typo gives two violations, the name that is missing and the name that is
// not known, because either alone sends you to the wrong place.
const FAILING = configTot
  .replace('sample-rate 0.25', 'sample-rate "0.25"')
  .replace('regions [', 'regoins [');

const samples: Record<Mode, { label: string; apply: (state: State) => void }[]> = {
  convert: [
    {
      label: 'a tour of the syntax',
      apply: (state) => void (state.convert.source = configTot),
    },
    { label: 'a JSON file', apply: (state) => void (state.convert.source = JSON_SAMPLE) },
  ],
  schema: [
    {
      label: 'a document that passes',
      apply: (state) => {
        state.schema.document = configTot;
        state.schema.shape = configSchemaTot;
      },
    },
    {
      label: 'a document that fails',
      apply: (state) => {
        state.schema.document = FAILING;
        state.schema.shape = configSchemaTot;
      },
    },
  ],
  template: [
    {
      label: 'all seven forms',
      apply: (state) => {
        state.template.files = {
          'service.tott': serviceTott,
          'defaults.tot': defaultsTot,
          'regions.tot': regionsTot,
        };
        state.template.entry = 'service.tott';
        state.template.active = 'service.tott';
        state.template.params = [
          { name: 'prod', value: 'true', raw: false },
          { name: 'tag', value: 'v1.4.2', raw: true },
        ];
      },
    },
  ],
};

function initialState(): State {
  const state: State = {
    mode: 'convert',
    convert: { source: configTot, target: 'json', compact: false },
    schema: { document: FAILING, shape: configSchemaTot },
    template: { files: {}, entry: 'service.tott', active: 'service.tott', params: [] },
  };
  samples.template[0].apply(state);
  return state;
}

// --- sharing -------------------------------------------------------------------------------

function encodeState(state: State): string {
  const bytes = new TextEncoder().encode(JSON.stringify(state));
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function decodeState(hash: string): State | null {
  try {
    const padded = hash.replace(/-/g, '+').replace(/_/g, '/');
    const binary = atob(padded);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    const parsed = JSON.parse(new TextDecoder().decode(bytes)) as State;
    // A shared link is someone else's input, so it is checked before it is trusted with the DOM.
    if (!parsed?.convert?.source && !parsed?.schema?.document) return null;
    return { ...initialState(), ...parsed };
  } catch {
    return null;
  }
}

// --- diagnostics ------------------------------------------------------------------------------

const escapeHtml = (s: string) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/**
 * Colours a caret diagnostic. The text is the CLI's, so this only recognises the shape it
 * already has: a severity, the `-->` locator, the `|` rail, and the carets.
 */
function paint(text: string): string {
  return escapeHtml(text)
    .replace(/^(error|warning|note)\b/gm, (word) => {
      const cls = word === 'error' ? 'te' : word === 'warning' ? 'tw' : 'tc';
      return `<span class="${cls}">${word}</span>`;
    })
    .replace(
      /^(\s*)(\d+)?(\s*)([|=])/gm,
      (_, before, number, gap, rail) =>
        `${before}${number ? `<span class="tc">${number}</span>` : ''}${gap}<span class="tp">${rail}</span>`,
    )
    .replace(/--&gt;/g, '<span class="tp">--&gt;</span>')
    .replace(/\^+/g, (carets) => `<span class="te">${carets}</span>`);
}

// --- editor -----------------------------------------------------------------------------------

class Editor {
  readonly input: HTMLTextAreaElement;
  private readonly layer: HTMLElement;
  private readonly gutter: HTMLElement;
  private readonly stat: HTMLElement | null;
  private readonly nameLabel: HTMLElement | null;
  private dialect: Dialect;
  private escaped = false;

  constructor(id: string, onChange: () => void) {
    const root = document.querySelector<HTMLElement>(`[data-editor="${id}"]`);
    if (!root) throw new Error(`no editor called ${id}`);

    this.input = root.querySelector('[data-editor-input]')!;
    this.layer = root.querySelector('[data-editor-highlight]')!;
    this.gutter = root.querySelector('[data-editor-gutter]')!;
    this.stat = root.querySelector('[data-editor-stat]');
    this.nameLabel = root.querySelector('[data-editor-name]');
    this.dialect = (root.dataset.dialect as Dialect) ?? 'tot';

    this.input.addEventListener('input', () => {
      this.paint();
      onChange();
    });

    // The layers only stay aligned if they scroll together.
    this.input.addEventListener('scroll', () => {
      this.layer.scrollTop = this.input.scrollTop;
      this.layer.scrollLeft = this.input.scrollLeft;
      this.gutter.scrollTop = this.input.scrollTop;
    });

    this.input.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        // Escape then Tab leaves the editor, which is the way out for anyone using a keyboard.
        this.escaped = true;
        return;
      }
      if (event.key !== 'Tab' || this.escaped) {
        this.escaped = false;
        return;
      }
      event.preventDefault();
      const { selectionStart, selectionEnd, value } = this.input;
      this.input.value = `${value.slice(0, selectionStart)}  ${value.slice(selectionEnd)}`;
      this.input.selectionStart = this.input.selectionEnd = selectionStart + 2;
      this.paint();
      onChange();
    });
  }

  get value(): string {
    return this.input.value;
  }

  set value(text: string) {
    this.input.value = text;
    this.paint();
  }

  setFile(name: string, dialect: Dialect) {
    this.dialect = dialect;
    if (this.nameLabel) this.nameLabel.textContent = name;
    this.input.setAttribute('aria-label', name);
  }

  paint() {
    const text = this.value;
    // The extra newline is what keeps a trailing blank line visible: <pre> swallows one.
    this.layer.innerHTML = highlight(`${text}\n`, this.dialect);

    const lines = text.split('\n').length;
    this.gutter.textContent = Array.from({ length: lines }, (_, i) => i + 1).join('\n');

    if (this.stat) {
      const bytes = new TextEncoder().encode(text).length;
      this.stat.textContent = `${lines} line${lines === 1 ? '' : 's'} · ${bytes} B`;
    }
  }
}

// --- the page ---------------------------------------------------------------------------------

export function start() {
  const root = document.querySelector<HTMLElement>('.play');
  if (!root) return;

  const state = (location.hash.length > 1 && decodeState(location.hash.slice(1))) || initialState();

  const drawerTitle = root.querySelector<HTMLElement>('[data-drawer-title]')!;
  const drawerCounts = root.querySelector<HTMLElement>('[data-drawer-counts]')!;
  const drawerStatus = root.querySelector<HTMLElement>('[data-drawer-status]')!;
  const drawerBody = root.querySelector<HTMLElement>('[data-drawer-body]')!;
  const sampleChips = root.querySelector<HTMLElement>('[data-samples]')!;
  const paramRows = root.querySelector<HTMLElement>('[data-param-rows]')!;
  const importRows = root.querySelector<HTMLElement>('[data-import-rows]')!;
  const templateTabs = root.querySelector<HTMLElement>('[data-template-tabs]')!;
  const templateStat = root.querySelector<HTMLElement>('[data-template-stat]')!;
  const convertOut = root.querySelector<HTMLElement>('[data-output="convert"]')!;
  const templateOut = root.querySelector<HTMLElement>('[data-output="template"]')!;
  const compactBox = root.querySelector<HTMLInputElement>('[data-action="compact"]')!;

  let ready = false;
  let timer: number | undefined;

  const editors = {
    convert: new Editor('convert-input', queue),
    document: new Editor('schema-document', queue),
    shape: new Editor('schema-shape', queue),
    template: new Editor('template-editor', queue),
  };

  function queue() {
    window.clearTimeout(timer);
    timer = window.setTimeout(run, 120);
  }

  // --- rendering helpers ---

  function showDiagnostics(title: string, blocks: string[], counts: string, clean: string) {
    drawerTitle.textContent = title;
    drawerCounts.innerHTML = counts;
    drawerBody.innerHTML = blocks.length
      ? `<div class="diags">${blocks.map((block) => `<pre>${block}</pre>`).join('')}</div>`
      : `<p class="clean">${clean}</p>`;
  }

  function dialectOf(file: string): Dialect {
    return file.toLowerCase().endsWith('.tott') ? 'tott' : 'tot';
  }

  function renderTemplateTabs() {
    templateTabs.innerHTML = '';
    for (const file of Object.keys(state.template.files)) {
      const button = document.createElement('button');
      button.type = 'button';
      button.role = 'tab';
      button.textContent = file;
      button.ariaSelected = String(file === state.template.active);
      button.addEventListener('click', () => {
        state.template.files[state.template.active] = editors.template.value;
        state.template.active = file;
        editors.template.setFile(file, dialectOf(file));
        editors.template.value = state.template.files[file] ?? '';
        renderTemplateTabs();
        run();
      });
      templateTabs.append(button);
    }
  }

  function renderParams() {
    paramRows.innerHTML = '';
    state.template.params.forEach((param, index) => {
      const row = document.createElement('div');
      row.className = 'param';

      const kind = document.createElement('button');
      kind.type = 'button';
      kind.className = 'kind';
      kind.textContent = param.raw ? '--set-raw' : '--set';
      kind.title = param.raw
        ? 'The text is taken as a string, with no quotes needed'
        : 'The text is parsed as a tot value, so a string needs its quotes';
      kind.addEventListener('click', () => {
        param.raw = !param.raw;
        renderParams();
        run();
      });

      const name = document.createElement('input');
      name.value = param.name;
      name.setAttribute('aria-label', 'Parameter name');
      name.addEventListener('input', () => {
        param.name = name.value;
        queue();
      });

      const value = document.createElement('input');
      value.value = param.value;
      value.setAttribute('aria-label', 'Parameter value');
      value.addEventListener('input', () => {
        param.value = value.value;
        queue();
      });

      const drop = document.createElement('button');
      drop.type = 'button';
      drop.className = 'drop';
      drop.textContent = '×';
      drop.setAttribute('aria-label', `Remove ${param.name || 'parameter'}`);
      drop.addEventListener('click', () => {
        state.template.params.splice(index, 1);
        renderParams();
        run();
      });

      row.append(kind, name, value, drop);
      paramRows.append(row);
    });
  }

  // --- the three modes ---

  function runConvert() {
    const source = editors.convert.value;
    state.convert.source = source;

    const target =
      state.convert.target === 'json' && state.convert.compact ? 'json-compact' : state.convert.target;
    const result = JSON.parse(wasmConvert(source, target)) as Result;

    if (result.ok) {
      convertOut.removeAttribute('data-stale');
      convertOut.innerHTML =
        state.convert.target === 'tot' || state.convert.target.startsWith('json')
          ? highlight(result.value ?? '', 'tot')
          : escapeHtml(result.value ?? '');
      // A note is not a warning: `tot to toml` drops a null and says so, and still reports a
      // clean document. Counting the two together would make every null look like a complaint.
      const warnings = result.warnings ?? [];
      const notes = result.notes ?? [];
      showDiagnostics(
        'Diagnostics',
        [...warnings, ...notes].map((diagnostic) => paint(diagnostic.render)),
        `<span class="warn">${warnings.length} warning${warnings.length === 1 ? '' : 's'}</span>` +
          (notes.length ? `<span>${notes.length} note${notes.length === 1 ? '' : 's'}</span>` : '') +
          '<span>0 errors</span>',
        'No errors, and nothing the strict lint objects to.',
      );
    } else {
      convertOut.setAttribute('data-stale', '');
      showDiagnostics(
        'Diagnostics',
        [paint(result.error)],
        '<span class="bad">1 error</span>',
        '',
      );
    }
  }

  function runSchema() {
    state.schema.document = editors.document.value;
    state.schema.shape = editors.shape.value;

    const result = JSON.parse(
      wasmCheckSchema(state.schema.document, state.schema.shape),
    ) as Result;

    if (!result.ok) {
      showDiagnostics(
        'Violations',
        [paint(result.error)],
        `<span class="bad">the ${result.where ?? 'document'} does not parse</span>`,
        '',
      );
      return;
    }

    const violations = result.violations ?? [];
    showDiagnostics(
      'Violations',
      violations.map((violation) => paint(violation.render)),
      violations.length
        ? `<span class="bad">${violations.length} reported</span><span>every one, not just the first</span>`
        : '<span>0 violations</span>',
      'The document matches its schema.',
    );
  }

  function runTemplate() {
    state.template.files[state.template.active] = editors.template.value;

    const result = JSON.parse(
      wasmBuild(
        JSON.stringify(state.template.files),
        state.template.entry,
        JSON.stringify(state.template.params),
      ),
    ) as Result;

    if (result.ok) {
      templateOut.removeAttribute('data-stale');
      templateOut.innerHTML = highlight(result.value ?? '', 'tot');

      const imports = result.imports ?? [];
      templateStat.textContent = `built · ${imports.length} import${imports.length === 1 ? '' : 's'}`;
      importRows.innerHTML = imports.length
        ? imports
            .map(
              (file) =>
                `<div class="import"><b>${escapeHtml(file.name)}</b><span>${file.reads} reference${file.reads === 1 ? '' : 's'} · built once · ${file.bytes} B</span></div>`,
            )
            .join('')
        : '<div class="import"><span>no imports in this build</span></div>';

      const warnings = result.warnings ?? [];
      showDiagnostics(
        'Diagnostics',
        warnings.map((warning) => paint(warning.render)),
        `<span class="warn">${warnings.length} warning${warnings.length === 1 ? '' : 's'}</span><span>0 errors</span>`,
        'The template builds.',
      );
    } else {
      templateOut.setAttribute('data-stale', '');
      templateStat.textContent = 'build failed';
      showDiagnostics('Diagnostics', [paint(result.error)], '<span class="bad">1 error</span>', '');
    }
  }

  function run() {
    if (!ready) return;
    try {
      if (state.mode === 'convert') runConvert();
      else if (state.mode === 'schema') runSchema();
      else runTemplate();
    } catch (error) {
      // A panic in the wasm traps, and the release profile aborts rather than unwinds, so the
      // instance is unusable from here on: every later call throws too. Without this the throw
      // escapes the keystroke handler and the page just stops responding, with the last good
      // output still on screen looking current. `ready` goes back to false so it stops trying,
      // and the panic hook has already put the real message in the console.
      ready = false;
      drawerStatus.textContent = 'the parser stopped — reload the page';
      showDiagnostics(
        'Diagnostics',
        [escapeHtml(String(error))],
        '<span class="bad">the parser stopped</span>',
        '',
      );
    }
  }

  // --- wiring ---

  function setMode(mode: Mode) {
    state.mode = mode;
    for (const section of root!.querySelectorAll<HTMLElement>('[data-mode]')) {
      section.hidden = section.dataset.mode !== mode;
    }
    for (const button of root!.querySelectorAll<HTMLElement>('[data-mode-button]')) {
      button.ariaSelected = String(button.dataset.modeButton === mode);
    }
    renderSamples();
    run();
  }

  function renderSamples() {
    sampleChips.innerHTML = '';
    for (const sample of samples[state.mode]) {
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = sample.label;
      button.addEventListener('click', () => {
        sample.apply(state);
        syncEditors();
        run();
      });
      sampleChips.append(button);
    }
  }

  function syncEditors() {
    editors.convert.value = state.convert.source;
    editors.document.value = state.schema.document;
    editors.shape.value = state.schema.shape;
    editors.template.setFile(state.template.active, dialectOf(state.template.active));
    editors.template.value = state.template.files[state.template.active] ?? '';
    compactBox.checked = state.convert.compact;
    renderTemplateTabs();
    renderParams();
    for (const button of root!.querySelectorAll<HTMLElement>('[data-target]')) {
      button.ariaSelected = String(button.dataset.target === state.convert.target);
    }
  }

  for (const button of root.querySelectorAll<HTMLElement>('[data-mode-button]')) {
    button.addEventListener('click', () => setMode(button.dataset.modeButton as Mode));
  }

  for (const button of root.querySelectorAll<HTMLElement>('[data-target]')) {
    button.addEventListener('click', () => {
      state.convert.target = button.dataset.target!;
      for (const other of root.querySelectorAll<HTMLElement>('[data-target]')) {
        other.ariaSelected = String(other === button);
      }
      run();
    });
  }

  compactBox.addEventListener('change', () => {
    state.convert.compact = compactBox.checked;
    run();
  });

  root.querySelector('[data-action="add-param"]')?.addEventListener('click', () => {
    state.template.params.push({ name: '', value: '', raw: false });
    renderParams();
  });

  for (const button of root.querySelectorAll<HTMLElement>('[data-action="copy-output"]')) {
    button.addEventListener('click', async () => {
      const pane = button.closest('.pane')?.querySelector('.output-body');
      try {
        await navigator.clipboard.writeText(pane?.textContent ?? '');
        button.textContent = 'copied';
        setTimeout(() => (button.textContent = 'copy'), 1400);
      } catch {
        button.textContent = 'select and copy';
      }
    });
  }

  function formatActive() {
    if (!ready) return;
    const targets: [Editor, Dialect][] =
      state.mode === 'convert'
        ? [[editors.convert, 'tot']]
        : state.mode === 'schema'
          ? [
              [editors.document, 'tot'],
              [editors.shape, 'tot'],
            ]
          : [[editors.template, dialectOf(state.template.active)]];

    for (const [editor, dialect] of targets) {
      const result = JSON.parse(wasmFormat(editor.value, dialect === 'tott')) as Result;
      // A document that does not parse cannot be formatted; the diagnostic already says so.
      if (result.ok && result.value !== undefined) editor.value = result.value;
    }
    run();
  }

  root.querySelector('[data-action="format"]')?.addEventListener('click', formatActive);

  document.addEventListener('keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      formatActive();
    }
  });

  const shareLabel = root.querySelector<HTMLElement>('[data-share-label]')!;
  root.querySelector('[data-action="share"]')?.addEventListener('click', async () => {
    state.convert.source = editors.convert.value;
    state.schema.document = editors.document.value;
    state.schema.shape = editors.shape.value;
    state.template.files[state.template.active] = editors.template.value;

    const url = `${location.origin}${location.pathname}#${encodeState(state)}`;
    history.replaceState(null, '', url);
    try {
      await navigator.clipboard.writeText(url);
      shareLabel.textContent = 'link copied';
    } catch {
      shareLabel.textContent = 'link in the address bar';
    }
    setTimeout(() => (shareLabel.textContent = 'Share'), 1600);
  });

  // --- boot ---

  syncEditors();
  setMode(state.mode);

  init()
    .then(() => {
      ready = true;
      drawerStatus.textContent = 'tot 0.1.0 · WebAssembly · nothing leaves your browser';
      run();
    })
    .catch((error: unknown) => {
      drawerStatus.textContent = 'the parser failed to load';
      drawerBody.innerHTML = `<p class="clean">${escapeHtml(String(error))}</p>`;
    });
}
