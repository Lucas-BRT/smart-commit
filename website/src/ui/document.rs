use topcoat::{
    Result,
    view::{Child, View, component, view},
};

use super::{footer::site_footer, header::site_header};

/// The page around every page: the head, the skip link, the header, the
/// main column and the footer.
#[component]
pub async fn document(
    title: &str,
    description: &str,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>(title)</title>
                <meta name="description" content=(description)>
                <meta name="color-scheme" content="dark">
                <meta name="theme-color" content="#0d0c0b">
                <link rel="icon" href="/logo.svg" type="image/svg+xml">
                <link rel="stylesheet" href="/style.css">
                topcoat::dev::script()
            </head>
            <body class="bg-background text-foreground">
                <a class="absolute -top-16 left-4 z-10 rounded-lg bg-primary px-4 py-2 text-primary-foreground focus:top-4 print:hidden" href="#main">"Skip to content"</a>
                site_header()
                <main id="main">(child)</main>
                site_footer()
            </body>
        </html>
    })
}
