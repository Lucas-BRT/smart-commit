use topcoat::{
    Result,
    view::{Child, StaticClass, View, attributes, class, component, view},
};

use crate::components::table::{
    table, table_body, table_cell, table_head, table_header, table_row,
};

/// On a narrow screen the table stops being a table: each row is a block and
/// each cell shows the name of its column, which `cell` puts in `data-label`.
const NARROW_TABLE: StaticClass = class!("max-sm:block");
const NARROW_HEAD: StaticClass = class!("max-sm:sr-only");
const NARROW_BODY: StaticClass = class!("max-sm:block");
const NARROW_ROW: StaticClass = class!(
    "max-sm:mb-3 max-sm:block max-sm:rounded-xl max-sm:border max-sm:border-border",
    "max-sm:bg-card max-sm:px-4 max-sm:py-1 max-sm:last:mb-0"
);
const CELL: StaticClass = class!(
    "align-top whitespace-normal",
    "max-sm:block max-sm:px-0 max-sm:py-2 max-sm:first:pl-0 max-sm:last:pr-0",
    "max-sm:before:block max-sm:before:font-mono max-sm:before:text-xs max-sm:before:text-primary",
    "max-sm:before:content-[attr(data-label)] max-sm:[&+td]:border-t max-sm:[&+td]:border-border",
);

/// A table of text, in a bordered box. `head` names the columns; `label`
/// names the table for assistive technology. Pass `row`s as children.
#[component]
pub async fn data_table(
    label: &str,
    head: &[&str],
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <div
            class="my-4 mb-6 overflow-hidden rounded-xl border border-border bg-card max-sm:overflow-visible max-sm:border-0 max-sm:bg-transparent"
            tabindex="0"
            role="region"
            aria-label=(label)
        >
            table(
                attrs: attributes! { class=(NARROW_TABLE) },
                table_header(
                    attrs: attributes! { class=(NARROW_HEAD) },
                    table_row(
                        for name in head {
                            table_head(attrs: attributes! { scope="col" }, (*name))
                        }
                    )
                )
                table_body(attrs: attributes! { class=(NARROW_BODY) }, (child))
            )
        </div>
    })
}

/// One row of a [`data_table`].
#[component]
pub async fn row(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! { table_row(attrs: attributes! { class=(NARROW_ROW) }, (child)) })
}

/// One cell of a [`row`]; `label` is the name of its column.
#[component]
pub async fn cell(label: &str, #[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        table_cell(attrs: attributes! { class=(CELL) data-label=(label) }, (child))
    })
}
