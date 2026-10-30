# smart-commit website

Marketing and documentation site for smart-commit, written in Rust with
Topcoat (pinned to `=0.11.0`). Server-rendered HTML, no JavaScript. The
stylesheet is built by `build.rs` from `styles.css` (the `topcoat ui` theme,
retuned to the site palette): Tailwind with its reset, the utilities for the
class names in `src/**/*.rs`, and the hand-written `assets/style.css` in the
`components` layer. New styling uses Tailwind utilities and the components in
`src/components` (`topcoat ui add NAME`); `style.css` shrinks as pages move
over. A class name in `style.css` that Tailwind also defines (`grid`, `hidden`)
breaks the utility, so delete the old rule when a page moves. Run it with `topcoat dev` (reload on
change) or `cargo run -p smart-commit-website`; set `HOST=0.0.0.0` to open it from a phone.

## Done means checked

Every change to the frontend (markup, copy that changes the layout, CSS, the
logo or any asset) is checked visually on three devices before you say it is
finished:

| Device | Width |
|---|---|
| Desktop | 1280 px |
| Tablet | 820 px |
| Mobile | 390 px |

1. Start the site, then run `scripts/visual-check.sh /path ...` with every page
   the change can reach (a change to the layout, the header or the stylesheet
   reaches all of them).
2. Open each screenshot and look at it. Do not assume.
3. Look for: horizontal overflow, clipped or wrapped-badly text, orphaned items
   in a grid, uneven spacing, misaligned items, controls that are hidden or too
   small to tap, and contrast.
4. Open states count too: check the states a change can alter (a menu, a
   scrolled table, a focused control).
5. Fix what you find and check again. Only then say it is done.
6. In the report, say which pages and sizes you checked, and which you did not.

The site has one theme, dark; there is no light theme to check.

`cargo fmt --check`, `cargo clippy --all-targets -p smart-commit-website -- -D warnings` and
`cargo test -p smart-commit-website` must pass too, but they do not replace the visual check: the tests
cannot see a layout.

## Style rules

- Use the `frontend-design` skill for any visual change, and read its output
  against the page that exists before editing.
- No fixed lengths in CSS: use `rem`, `em`, `%`, `fr`, `clamp()` and similar.
  Breakpoints are in `rem`.
- The landing page (`/`) may sell. Every other page is documentation for
  developers: plain titles, no slogans, no calls to action.
- Facts, numbers and commands come from the smart-commit documentation
  (`docs/` at the root of this repository). Never invent a number or a command.
- The product runs on Linux only. Do not use macOS conventions in examples or
  in the terminal blocks.
- Keep the page in step with the tests in `tests/routes.rs`: a new page gets a
  route test, and a copy change that a test names changes the test.
