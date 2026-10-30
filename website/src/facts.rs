//! What the site says about the product, read from the product's own files
//! by `build.rs`: the version, the repository, the install command, the
//! default model, the devices and the next roadmap items.

include!(concat!(env!("OUT_DIR"), "/facts.rs"));

/// A page of the product's documentation, on GitHub.
pub fn docs_url(page: &str) -> String {
    format!("{REPO}/blob/main/docs/{page}")
}
