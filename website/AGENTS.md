# smart-commit website

Read [../AGENTS.md](../AGENTS.md) first: commits, tests, documentation and
reproducibility apply here as everywhere. This file adds what is only about the
site.

Marketing and documentation site, written in Rust with Topcoat (pinned to
`=0.11.0`). Server-rendered HTML, no JavaScript, one theme (dark, one violet accent). Run it with
`cargo run -p smart-commit-website` (or `topcoat dev`, which reloads on
change); `HOST=0.0.0.0` opens it from a phone. `docker compose up -d --build`
here runs the image.

## Where things come from

- **Facts come from the product, by build.** The version, repository, install
  command, default model and its size, the devices table, the benchmark example
  and the next roadmap items are read by `build.rs` from `../cli/Cargo.toml`,
  `../cli/src/models/registry.rs` and `../docs/{devices,usage,roadmap}.md`, into
  `facts`. Never type one of them into a page: change the source file.
  The measurements page is the exception that needs care: the numbers it draws
  are copied from `../docs/measurements.md` into `src/measures.rs`, and
  `tests/routes.rs` reads that document and looks for each one on the page, so a
  changed measurement fails a test until `measures.rs` follows. Charts are
  HTML and CSS only: the bar decorates, the number is text.
  Anything else the pages say is written by hand: check it against `../docs/`
  and the CLI when the product changes, and never invent a number or a command.
- **Styles:** `styles.css` is the Tailwind entry (the `topcoat ui` theme, with
  the site palette: near-black, violet accent, Bricolage Grotesque headings); `build.rs` builds
  it with the utilities used in `src/**/*.rs` and the hand-written
  `assets/style.css`. Layout and spacing are Tailwind utilities in the Rust
  sources; `style.css` holds only the font and the defaults of plain elements (type,
  lists, `code`); the terminal, tables, steps and charts are components in `src/ui`. No animations or effects. The display font is a file of the site
  (`assets/fonts`, SIL OFL), never a request to a font host.
  A class in `style.css` that Tailwind also defines (`grid`, `hidden`) breaks the utility.
  After editing a CSS file, touch `build.rs` if the served stylesheet looks
  stale: Cargo does not always rerun it.

## Code layout

```
src/
  app.rs            routes: the root layout, the home page, mod declarations
  app/<page>.rs     a page: only the `#[page]` that composes its sections
  app/<page>/*.rs   one component per section (`pub async fn section()`)
  ui/               components of this site, built on the ones below
  components/       Topcoat UI, copied with `topcoat ui add` (see components.toml)
  site.rs           every page in one table: menu, title, description, section
  assets.rs         the stylesheet, the mark and the font, as routes
  facts.rs          what the site reads from the product (build.rs)
  measures.rs       the numbers of docs/measurements.md the charts draw
```

A page with sections is a folder; split a page when it has several sections,
not by size. A module with no `#[page]` adds no route. New page: the module,
a row in `site::PAGES` (its handler is its identity, never its path), a row in
`tests/routes.rs::PAGES`.

## Topcoat

Read [Topcoat's `llms.txt`](https://raw.githubusercontent.com/tokio-rs/topcoat/refs/heads/main/llms.txt)
before writing markup. It describes the `main` branch; the site is pinned to
`=0.11.0`, whose guides are in `~/.cargo/registry/src/*/topcoat-0.11.0/docs` and
`topcoat-view-macro-0.11.0/docs`. Route context (`with(...)`), `Metadata` and
`link` are not in 0.11.0. What the site relies on:

- **Reusable markup is a component** (`#[component]`), called as `card(item: x)`,
  not a function that returns markup. Props are typed: enums for variants,
  `&str` for text, `#[default]` for what may be left out, `child: Child` for content.
- **Classes:** a fixed list is `class="..."`; a composed one is `class!(...)`, a
  constant is `StaticClass`. Never `format!` a class, and never a `&str` constant
  of classes. A component that wraps one element takes `attrs: Attributes` and
  merges with `class!(BASE, attrs.remove("class"))`; Tailwind does not resolve
  conflicts, so a caller cannot override a class the component already sets.
- **Values:** interpolate numbers directly, `(count)`; a value that is not text
  gets its own type (`ui::chart::Share` writes a `width:` style). `let` values
  that a `view!` borrows are computed before it.
- **Links:** `href!(handler)`, with `.fragment("id")` for an anchor and
  `.is_current(cx)` for the page you are on. Only external URLs are strings.
- **No JavaScript.** Do not add Topcoat UI components that need the runtime
  (`sheet`, `sidebar`, `navigation_menu`, `dialog`, `tabs`, `alert_dialog`), and
  skip `suspense`, `live!` and `emit!`: every page is rendered whole.
- **Icons** are `IconData` constants (`ui/icons.rs`) rendered with `icon(...)`.
- `topcoat fmt` formats `view!` bodies, but the installed CLI can be older than
  the crate (its grammar is 0.5, the site's is 0.11) and fail; run `cargo fmt`
  always, `topcoat fmt` only when it works.

## Done means checked

`cargo fmt --check`, `cargo clippy --all-targets -p smart-commit-website -- -D
warnings` and `cargo test -p smart-commit-website` must pass, but tests cannot
see a layout. Every change to markup, copy that changes the layout, CSS, the logo
or an asset is also checked visually, on three devices:

| Device | Width |
|---|---|
| Desktop | 1280 px |
| Tablet | 820 px |
| Mobile | 390 px |

1. Start the site, then run `scripts/visual-check.sh /path ...` with every page
   the change can reach (a change to the layout, the header or the stylesheet
   reaches all of them).
2. Open each screenshot and look at it. Do not assume.
3. Look for horizontal overflow, clipped or badly wrapped text, orphaned items
   in a grid, uneven spacing, misalignment, controls that are hidden or too
   small to tap, contrast, and the states a change can alter (a menu, a scrolled
   table, a focused control).
4. Fix what you find and check again. Only then say it is done.
5. In the report, say which pages and sizes you checked, and which you did not.

## Style rules

- Use the `frontend-design` skill for any visual change, and read its output
  against the page that exists before editing.
- No fixed lengths in CSS: use `rem`, `em`, `%`, `fr`, `clamp()` and similar.
  Breakpoints are in `rem`.
- The landing page (`/`) may sell. Every other page is documentation for
  developers: plain titles, no slogans, no calls to action.
- The product runs on Linux only: no macOS conventions in examples or terminal
  blocks.
- Say only what holds: writing a message is local, but installing, updating and
  downloading models use the network.
- Tests check the site against the product, never its copy: a fact (version,
  repository, installer, models, devices, measurements) must match the file
  that says it; every command, setting and error message a page shows must exist
  in the documents or the sources; every link must resolve, fragments included.
  A new page is added to `PAGES` in `tests/routes.rs`, which runs those checks on
  it. Do not add a test that reads a sentence or a heading: rewording a page must
  not need a test change.
