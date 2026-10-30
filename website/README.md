# smart-commit website

Marketing and documentation site for [smart-commit](https://github.com/Lucas-BRT/smart-commit),
written in Rust with [Topcoat](https://github.com/tokio-rs/topcoat) (pinned to
`=0.11.0`). Server-rendered HTML, one stylesheet, no JavaScript, no external
fonts, CDNs or trackers. The site is dark only.

## Run

```bash
cargo run -p smart-commit-website                  # http://127.0.0.1:3000 (override with HOST and PORT)
PORT=8080 cargo run -p smart-commit-website
```

Topcoat's own dev server (rebuild and reload on change) is optional:
`cargo install topcoat-cli`, then `topcoat dev`.

## Docker

```bash
docker compose up -d --build     # http://localhost:3000 (PORT=8080 docker compose up -d for another port)
```

The image is a static musl binary on `scratch` (about 4.5 MB): no shell, no root
user, read-only filesystem, all capabilities dropped. The binary doubles as its
own healthcheck (`/server --health`). Compression and TLS belong to the CDN or
reverse proxy in front of it; the app only sets `Cache-Control` on the stylesheet.

## Checks

```bash
cargo fmt --check
cargo clippy --all-targets -p smart-commit-website -- -D warnings
cargo test -p smart-commit-website
```

## Layout

Routes come from the module tree (Topcoat's module router):

| File | URL |
|---|---|
| `src/app.rs` | `/` (home), the root layout, `/style.css`, the 404 page |
| `src/app/install.rs` | `/install` |
| `src/app/devices.rs` | `/devices` |
| `src/app/how_it_works.rs` | `/how-it-works` |
| `src/app/tutorial.rs` | `/tutorial` |
| `src/app/roadmap.rs` | `/roadmap` |
| `src/ui.rs` | shared components (terminal, table, page head) |
| `assets/style.css` | the stylesheet, embedded in the binary |

## Facts from the product

`build.rs` reads these files of the repository and writes the facts to
`facts.rs` (`src/ui.rs`, module `facts`):

| Fact | Source |
|---|---|
| version (footer, install page) and repository | `cli/Cargo.toml` |
| default model and its size (install page) | `cli/src/models/registry.rs` |
| devices table | the first table of `docs/devices.md` |
| install command | the first `curl` line of `docs/usage.md` |
| next items of the roadmap | the `Next` table of `docs/roadmap.md` |

A change to one of them rebuilds the site, and a file that no longer parses
stops the build. `tests/routes.rs` reads the same files on its own, so a page
that drifts from the product fails a test. Anything else the pages say is
written by hand and has to be checked against `docs/` when the product changes.

## Content

Written from the repository's `README.md`, `docs/usage.md`,
`docs/devices.md`, `docs/architecture.md` and `docs/roadmap.md` (one level up). Facts, commands
and the example outputs are taken from those pages; the benchmark output is the
example they show (a Ryzen 5 5500 with an RTX 3050), not a new measurement.
The facts above follow the product by themselves; the rest of the text is
written by hand, so check it when the docs change.
