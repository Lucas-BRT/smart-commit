//! The few icons of the site, as Topcoat `IconData`, drawn on a 24-unit grid
//! in the current text color.

use topcoat::{
    Result,
    icon::{IconData, icon},
    view::{View, attributes, component, svg::ViewBox, view},
};

/// An outline icon: `$body` drawn with a round `$stroke`-unit line.
macro_rules! outline {
    ($stroke:literal, $body:literal) => {
        IconData::unescaped_unchecked(
            ViewBox::new(0.0, 0.0, 24.0, 24.0),
            concat!(
                r#"<g fill="none" stroke="currentColor" stroke-width=""#,
                $stroke,
                r#"" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</g>"
            ),
        )
    };
}

pub const GPU: IconData = outline!(
    "1.6",
    r#"<rect x="2" y="6" width="20" height="11" rx="2"/><circle cx="9" cy="11.5" r="2.5"/><path d="M15 9.5h4M15 13.5h4M6 17v2.5M10 17v2.5M14 17v2.5"/>"#
);

pub const NPU: IconData = outline!(
    "1.6",
    r#"<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2.5V6M15 2.5V6M9 18v3.5M15 18v3.5M2.5 9H6M2.5 15H6M18 9h3.5M18 15h3.5"/><path d="M12 9l.9 2.1L15 12l-2.1.9L12 15l-.9-2.1L9 12l2.1-.9z"/>"#
);

pub const CPU: IconData = outline!(
    "1.6",
    r#"<rect x="5" y="5" width="14" height="14" rx="2"/><rect x="9" y="9" width="6" height="6" rx="1"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/>"#
);

/// A person who is not there: nothing to sign up for.
pub const NO_ACCOUNT: IconData = outline!(
    "1.8",
    r#"<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.6-3.6 3.3-5.5 6.5-5.5 1.3 0 2.5.3 3.5.9"/><path d="M15.5 15.5l6 6M21.5 15.5l-6 6"/>"#
);

/// A box: a model that is a file to download.
pub const PACKAGE: IconData = outline!(
    "1.8",
    r#"<path d="M12 2.5l8.5 4.7v9.6L12 21.5l-8.5-4.7V7.2z"/><path d="M12 12l8.5-4.8M12 12L3.5 7.2M12 12v9.5"/>"#
);

/// A signal crossed out: no network.
pub const OFFLINE: IconData = outline!(
    "1.8",
    r#"<path d="M2 8.8a15 15 0 0 1 4.2-2.6M22 8.8a15 15 0 0 0-8.6-3.7M5 12.6a10 10 0 0 1 3-1.9M19 12.6a10 10 0 0 0-3.2-2M8.5 16.2a5 5 0 0 1 7 0"/><path d="M12 19.8h.01M3 3l18 18"/>"#
);

/// An icon in a small square of its own, for a list of features.
#[component]
pub async fn icon_badge(data: IconData) -> Result<impl View> {
    Ok(view! {
        <span class="grid size-10 shrink-0 place-items-center rounded-lg bg-background text-primary">
            icon(data: data, size: 20)
        </span>
    })
}

/// An icon for the head of a card.
#[component]
pub async fn card_icon(data: IconData) -> Result<impl View> {
    Ok(view! {
        icon(data: data, size: 28, attrs: attributes! { class="mb-2 text-primary" })
    })
}
