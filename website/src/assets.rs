//! The files the site serves besides its pages: the stylesheet, the mark and
//! the display face. All three are part of the binary.

use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use topcoat::{Result, router::response::IntoResponse, router::route};

/// Built by `build.rs`: Tailwind's utilities plus `assets/style.css`.
const STYLE: &str = include_str!(concat!(env!("OUT_DIR"), "/tailwind.css"));
const LOGO: &str = include_str!("../assets/logo.svg");
/// Bricolage Grotesque (SIL OFL, `assets/fonts/OFL.txt`).
const FONT: &[u8] = include_bytes!("../assets/fonts/bricolage-grotesque-latin.woff2");

/// Debug builds (what `topcoat dev` runs) never cache, so a phone sees each edit on reload.
const CACHE: &str = if cfg!(debug_assertions) {
    "no-store"
} else {
    "public, max-age=3600"
};

#[route(GET "/style.css")]
async fn style() -> Result<impl IntoResponse> {
    Ok((
        [
            (CONTENT_TYPE, "text/css; charset=utf-8"),
            (CACHE_CONTROL, CACHE),
        ],
        STYLE,
    ))
}

/// The mark: favicon and header logo.
#[route(GET "/logo.svg")]
async fn logo() -> Result<impl IntoResponse> {
    Ok((
        [(CONTENT_TYPE, "image/svg+xml"), (CACHE_CONTROL, CACHE)],
        LOGO,
    ))
}

/// The display face, served from the site so no request leaves for a font host.
#[route(GET "/fonts/bricolage-grotesque-latin.woff2")]
async fn font() -> Result<impl IntoResponse> {
    Ok(([(CONTENT_TYPE, "font/woff2"), (CACHE_CONTROL, CACHE)], FONT))
}
