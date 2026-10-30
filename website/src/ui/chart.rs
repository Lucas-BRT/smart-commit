//! Charts made of HTML and CSS: no script, no image. A bar only decorates;
//! every number is also text.

use topcoat::{
    Result,
    context::Cx,
    view::{
        AttributeValueViewParts, Attributes, Child, PartsWriter, StaticClass, View, class,
        component, view,
    },
};

/// What a bar stands for, and so what it is filled with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// The baseline, or the cost to avoid.
    #[default]
    Quiet,
    /// The result to look at.
    Accent,
    /// A third thing, where there are three.
    Ink,
}

impl Tone {
    fn fill(self) -> StaticClass {
        match self {
            Self::Quiet => class!("bg-muted-foreground/45"),
            Self::Accent => class!("bg-primary"),
            Self::Ink => class!("bg-foreground/70"),
        }
    }
}

/// A width as a share of its track, written as an inline `width:` style,
/// rounded to a tenth of a percent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Share(f32);

impl Share {
    /// `value` out of `max`, never outside the track.
    pub fn of(value: f64, max: f64) -> Self {
        let percent = (value / max * 100.0).clamp(0.0, 100.0);
        Self(((percent * 10.0).round() / 10.0) as f32)
    }
}

impl AttributeValueViewParts for Share {
    fn attribute_present(&self) -> bool {
        true
    }

    fn into_view_parts(self, _cx: &Cx, parts: &mut PartsWriter<'_>) {
        parts.push_static_str("width:");
        parts.push_f32(self.0);
        parts.push_static_str("%");
    }
}

/// A part of a stacked bar: how much, and of what.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub amount: f64,
    pub tone: Tone,
}

const TRACK: StaticClass = class!("block h-3 rounded-full bg-card");
const ROW: StaticClass = class!(
    "grid grid-cols-[minmax(0,1fr)_4.5rem] items-center gap-x-3",
    "sm:grid-cols-[14rem_minmax(0,1fr)_5rem]",
);

/// One row of a bar chart: the name, a bar as wide as `value` is of `max`,
/// and the number as text. An empty `name` leaves the row without one, to
/// pair it with the row above.
#[component]
pub async fn bar(
    name: &str,
    value: f64,
    max: f64,
    text: &str,
    #[default] tone: Tone,
) -> Result<impl View> {
    Ok(view! {
        <div class=(ROW)>
            <span
                class="col-span-2 text-sm text-muted-foreground [overflow-wrap:anywhere] max-sm:empty:hidden sm:col-span-1"
            >
                (name)
            </span>
            <span class=(TRACK) aria-hidden="true">
                <span
                    class=(class!("block h-full rounded-full", tone.fill()))
                    style=(Share::of(value, max))
                ></span>
            </span>
            <span class="text-right font-mono text-sm">(text)</span>
        </div>
    })
}

/// Rows of one group, close together.
#[component]
pub async fn bar_group(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! { <div class="grid gap-1.5">(child)</div> })
}

/// The meaning of a tone, above a chart.
#[component]
pub async fn swatch(tone: Tone, label: &str) -> Result<impl View> {
    Ok(view! {
        <span class="inline-flex items-center gap-2">
            <span
                class=(class!("inline-block size-3 rounded-full", tone.fill()))
                aria-hidden="true"
            ></span>
            (label)
        </span>
    })
}

/// The swatches of a chart.
#[component]
pub async fn legend(#[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <p
            class="m-0 mb-3 flex flex-wrap gap-x-6 gap-y-1 text-sm text-muted-foreground"
        >
            (child)
        </p>
    })
}

/// A bar of several parts, each as wide as its amount is of `max`, under a
/// caption and with the total as text. `apart` leaves a gap between the
/// parts, for separate events; otherwise they form one bar.
#[component]
pub async fn stacked(
    caption: &str,
    total: &str,
    max: f64,
    parts: &[Part],
    #[default] apart: bool,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <div>
            <div class="mb-1 flex justify-between gap-4 text-sm text-muted-foreground">
                <span>(caption)</span>
                <span class="font-mono whitespace-nowrap">(total)</span>
            </div>
            <div class=(class!("flex", "gap-0.5" if apart)) aria-hidden="true">
                for part in parts {
                    <span
                        class=(class!(
                            "block h-6",
                            part.tone.fill(),
                            "rounded" if apart else "first:rounded-l last:rounded-r",
                        ))
                        style=(Share::of(part.amount, max))
                    ></span>
                }
            </div>
            (child)
        </div>
    })
}

/// A chart, named for assistive technology. `attrs` adjust its layout.
#[component]
pub async fn chart(
    label: &str,
    #[default] mut attrs: Attributes,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <figure
            class=(class!("m-0 mt-6 grid gap-3", attrs.remove("class")))
            aria-label=(label)
            (attrs)
        >
            (child)
        </figure>
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_a_tenth_of_a_percent_inside_its_track() {
        assert_eq!(Share::of(5.0, 10.0), Share(50.0));
        assert_eq!(Share::of(1.0, 3.0), Share(33.3));
        assert_eq!(Share::of(20.0, 10.0), Share(100.0));
        assert_eq!(Share::of(-1.0, 10.0), Share(0.0));
    }
}
