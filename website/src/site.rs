//! The pages of the site, in one table: the navigation, each page's title
//! and its description come from here, and a page is found by its handler,
//! never by its path.

use topcoat::{
    context::Cx,
    router::{HrefTarget, href},
};

use crate::app::{
    self, devices, how_it_works, install, measurements, roadmap, settings, troubleshooting,
    tutorial,
};

/// A group of documentation pages in the side list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Start,
    Guides,
    Reference,
    Project,
}

impl Section {
    pub const ALL: [Self; 4] = [Self::Start, Self::Guides, Self::Reference, Self::Project];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Guides => "Guides",
            Self::Reference => "Reference",
            Self::Project => "Project",
        }
    }
}

pub struct Page {
    pub target: &'static (dyn HrefTarget + Sync),
    /// The name in the side list.
    pub label: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// The group of the side list, for a documentation page.
    pub section: Option<Section>,
}

pub static PAGES: [Page; 9] = [
    Page {
        target: &app::page,
        label: "Home",
        title: "smart-commit: commit messages in your repo's style, run locally",
        description: "Commit messages in your repo's style, from a small local model.",
        section: None,
    },
    Page {
        target: &tutorial::page,
        label: "Tutorial",
        title: "Your first commit - smart-commit",
        description: "Install smart-commit, stage, review and commit.",
        section: Some(Section::Start),
    },
    Page {
        target: &install::page,
        label: "Install",
        title: "Install and use smart-commit",
        description: "Install smart-commit and write your first message.",
        section: Some(Section::Start),
    },
    Page {
        target: &settings::page,
        label: "Settings and models",
        title: "Settings and models - smart-commit",
        description: "Change settings and choose a model.",
        section: Some(Section::Guides),
    },
    Page {
        target: &troubleshooting::page,
        label: "Troubleshooting",
        title: "Troubleshooting - smart-commit",
        description: "Check an installation and fix common errors.",
        section: Some(Section::Guides),
    },
    Page {
        target: &devices::page,
        label: "Devices",
        title: "Devices: NPU, GPU and CPU - smart-commit",
        description: "The hardware smart-commit runs on.",
        section: Some(Section::Reference),
    },
    Page {
        target: &measurements::page,
        label: "Measurements",
        title: "Measurements - smart-commit",
        description: "How long a message takes, by device and model.",
        section: Some(Section::Reference),
    },
    Page {
        target: &how_it_works::page,
        label: "How it works",
        title: "How smart-commit works",
        description: "From your history and diff to the commit message.",
        section: Some(Section::Reference),
    },
    Page {
        target: &roadmap::page,
        label: "Roadmap",
        title: "Roadmap - smart-commit",
        description: "What comes next.",
        section: Some(Section::Project),
    },
];

/// What a request for a page that does not exist is answered with.
pub const NOT_FOUND: (&str, &str) = ("Page not found - smart-commit", "This page does not exist.");

/// The page this request is for, if it is one of ours.
pub fn current(cx: &Cx) -> Option<&'static Page> {
    PAGES.iter().find(|page| href!(page.target).is_current(cx))
}

/// The documentation pages of a group, in order.
pub fn pages_in(section: Section) -> impl Iterator<Item = &'static Page> {
    PAGES
        .iter()
        .filter(move |page| page.section == Some(section))
}

/// The documentation page after the one this request is for, in the order of
/// the side list. The last page has none.
pub fn next(cx: &Cx) -> Option<&'static Page> {
    let here = current(cx)?;
    let mut order = Section::ALL.into_iter().flat_map(pages_in);
    order.find(|page| std::ptr::eq(*page, here))?;
    order.next()
}
