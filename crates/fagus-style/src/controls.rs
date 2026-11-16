//! The parts of `<progress>` and `<meter>` that Chrome builds in their shadow trees and styles
//! with pseudo-elements: the track (`::-webkit-progress-bar`, `::-webkit-meter-bar`) and the
//! filled part inside it, whose width the element sets in the part's `style` attribute.

use std::sync::LazyLock;

use fagus_css::matching::PseudoElementKind;
use fagus_dom::ElementData;
use lightningcss::properties::Property;
use lightningcss::properties::size::Size as CssSize;
use lightningcss::stylesheet::StyleAttribute;
use lightningcss::values::length::LengthPercentage as CssLengthPercentage;
use lightningcss::values::percentage::Percentage;

/// How good the value of a meter is (HTML, §4.10.14): it decides which pseudo-element styles the
/// filled part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterRegion {
    /// In the region of the optimum.
    Optimum,
    /// In a region next to that of the optimum.
    Suboptimum,
    /// Two regions away from that of the optimum.
    EvenLessGood,
}

impl MeterRegion {
    /// The pseudo-element that styles the filled part of a meter with a value in this region.
    pub(crate) fn pseudo_element(self) -> PseudoElementKind {
        match self {
            Self::Optimum => PseudoElementKind::MeterOptimumValue,
            Self::Suboptimum => PseudoElementKind::MeterSuboptimumValue,
            Self::EvenLessGood => PseudoElementKind::MeterEvenLessGoodValue,
        }
    }
}

/// A number attribute of an element, if it is a finite number.
fn number(data: &ElementData, name: &str) -> Option<f32> {
    data.attribute(name)
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
}

/// The fraction of a `<meter>` that its value fills, and the region of the value.
pub fn meter_value(data: &ElementData) -> (f32, MeterRegion) {
    let min = number(data, "min").unwrap_or(0.0);
    let max = number(data, "max").unwrap_or(1.0).max(min);
    let value = number(data, "value").unwrap_or(0.0).clamp(min, max);
    let low = number(data, "low").unwrap_or(min).clamp(min, max);
    let high = number(data, "high").unwrap_or(max).clamp(low, max);
    let optimum = number(data, "optimum")
        .unwrap_or(f32::midpoint(min, max))
        .clamp(min, max);
    let fraction = if max > min {
        (value - min) / (max - min)
    } else {
        0.0
    };
    // The regions: below low, between low and high, above high; the one with the optimum is
    // good, its neighbours suboptimal, the region two away even less good.
    let region = |point: f32| -> u8 {
        if point < low {
            0
        } else if point > high {
            2
        } else {
            1
        }
    };
    let region = match region(value).abs_diff(region(optimum)) {
        0 => MeterRegion::Optimum,
        1 => MeterRegion::Suboptimum,
        _ => MeterRegion::EvenLessGood,
    };
    (fraction, region)
}

/// The fraction of a `<progress>` that its value fills; `None` where it is indeterminate.
pub fn progress_fraction(data: &ElementData) -> Option<f32> {
    let max = number(data, "max").filter(|max| *max > 0.0).unwrap_or(1.0);
    number(data, "value").map(|value| value.clamp(0.0, max) / max)
}

/// The declaration in the `style` attribute of the filled part: its width, a percentage of the
/// track (Chrome's `inline-size` there). Author declarations marked `!important` win over it.
pub(crate) fn value_width(fraction: f32) -> Property<'static> {
    Property::Width(CssSize::LengthPercentage(CssLengthPercentage::Percentage(
        Percentage(fraction),
    )))
}

/// Declarations that Chrome's user agent style sheet gives the parts of a meter with its native
/// look (`:-internal-shadow-host-has-non-auto-appearance`): a light gray, rounded track with a
/// thin border, and a value in one color for its region, other colors in the dark color scheme
/// (Chrome 157, measured).
type Declarations = LazyLock<Option<StyleAttribute<'static>>>;

static NATIVE_BAR: Declarations = LazyLock::new(|| {
    fagus_css::parse_declarations(
        "background: light-dark(#efefef, #3b3b3b); border-radius: 20px;
         border: thin solid light-dark(rgba(118, 118, 118, 0.3), #858585)",
    )
});
static NATIVE_OPTIMUM: Declarations =
    LazyLock::new(|| fagus_css::parse_declarations("background: light-dark(#107c10, #74b374)"));
static NATIVE_SUBOPTIMUM: Declarations =
    LazyLock::new(|| fagus_css::parse_declarations("background: light-dark(#ffb900, #f2c812)"));
static NATIVE_EVEN_LESS_GOOD: Declarations =
    LazyLock::new(|| fagus_css::parse_declarations("background: light-dark(#d83b01, #e98f6d)"));

/// The native declarations of a meter part, by its pseudo-element.
pub(crate) fn native_meter_part(kind: PseudoElementKind) -> &'static [Property<'static>] {
    let attribute = match kind {
        PseudoElementKind::MeterBar => &NATIVE_BAR,
        PseudoElementKind::MeterOptimumValue => &NATIVE_OPTIMUM,
        PseudoElementKind::MeterSuboptimumValue => &NATIVE_SUBOPTIMUM,
        PseudoElementKind::MeterEvenLessGoodValue => &NATIVE_EVEN_LESS_GOOD,
        _ => return &[],
    };
    attribute
        .as_ref()
        .map_or(&[], |attribute| attribute.declarations.declarations.as_slice())
}
