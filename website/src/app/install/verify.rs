use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{docs::doc_section, terminal::terminal};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "verify",
            title: "Check the download",
            <p>
                "The installer checks the archive against " <code>"SHA256SUMS"</code> "."
            </p>
            <p>
                "Releases after 0.14.0 also carry a GitHub build attestation. With the "
                <a href="https://cli.github.com">"GitHub CLI"</a>
                ":"
            </p>
            terminal(
                title: "verify",
                label: "Verify the build provenance of a download",
                "gh attestation verify smart-commit-x86_64-unknown-linux-gnu.tar.gz --repo Lucas-BRT/smart-commit"
            )
            <p>
                "Or add " <code>"--verify"</code> " to the installer."
            </p>
        )
    })
}
