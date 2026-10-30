# smart-commit website

Marketing and documentation site for [smart-commit](https://github.com/Lucas-BRT/smart-commit),
written in Rust with [Topcoat](https://github.com/tokio-rs/topcoat) (pinned to
`=0.11.0`). Server-rendered HTML, one stylesheet, no JavaScript, no external
fonts, CDNs or trackers. Light and dark themes follow the system setting.

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
| `src/app/roadmap.rs` | `/roadmap` |
| `src/ui.rs` | shared components (terminal, table, page head) |
| `assets/style.css` | the stylesheet, embedded in the binary |

## Content

Written from the repository's `README.md`, `docs/usage.md`,
`docs/devices.md`, `docs/architecture.md` and `docs/roadmap.md` (one level up). Facts, commands
and the example outputs are taken from those pages; the benchmark output is the
example they show (a Ryzen 5 5500 with an RTX 3050), not a new measurement.
When the docs change, update the pages by hand.
