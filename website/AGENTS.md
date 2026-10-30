# smart-commit website

Read [../AGENTS.md](../AGENTS.md) first: commits, tests, documentation and
reproducibility apply here as everywhere. This file adds what is only about the
site.

Marketing and documentation site, written in Rust with Topcoat (pinned to
`=0.11.0`). Server-rendered HTML, no JavaScript, one theme (dark). Run it with
`cargo run -p smart-commit-website` (or `topcoat dev`, which reloads on
change); `HOST=0.0.0.0` opens it from a phone. `docker compose up -d --build`
here runs the image.

## Where things come from

- **Facts come from the product, by build.** The version, repository, install
  command, default model and its size, the devices table, the benchmark example
  and the next roadmap items are read by `build.rs` from `../cli/Cargo.toml`,
  `../cli/src/models/registry.rs` and `../docs/{devices,usage,roadmap}.md`, into
  `ui::facts`. Never type one of them into a page: change the source file.
  Anything else the pages say is written by hand: check it against `../docs/`
  and the CLI when the product changes, and never invent a number or a command.
- **Styles:** `styles.css` is the Tailwind entry (the `topcoat ui` theme, retuned
  to the site palette); `build.rs` builds it with the utilities used in
  `src/**/*.rs` and the hand-written `assets/style.css` (the `components`
  layer). New styling uses Tailwind utilities and the components in
  `src/components` (`topcoat ui add NAME`); `style.css` shrinks as pages move
  over. A class in `style.css` that Tailwind also defines (`grid`, `hidden`)
  breaks the utility: delete the old rule when a page moves.

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
- A new page gets a route test in `tests/routes.rs`, and a copy change that a
  test names changes the test.
