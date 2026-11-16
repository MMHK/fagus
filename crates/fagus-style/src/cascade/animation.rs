//! CSS animations as Chrome prints them (CSS Animations 1): a document is printed at time zero
//! of its animations, so an element whose animation fades it in prints transparent, one that
//! slides in prints where it starts, and one with a negative `animation-delay` prints as far
//! into its animation as the delay says (Chrome 157, measured). The keyframes' declarations
//! cascade above all normal declarations and below the important ones (CSS Cascade 5, §6.1);
//! between two keyframes, colors and opacity are interpolated and other values flip at the
//! middle, as discrete values do.

use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::animation::{
    AnimationDirection, AnimationFillMode, AnimationIterationCount, AnimationName,
};
use lightningcss::properties::{Property, PropertyId};
use lightningcss::rules::keyframes::KeyframeSelector;
use lightningcss::values::easing::{EasingFunction, StepPosition};
use lightningcss::values::time::Time;
use lightningcss::vendor_prefix::VendorPrefix;

use super::{Declared, Expansions, Ordered, cascade_declarations, cascade_id, expand, resolve};
use crate::functions::Functions;
use crate::style::ComputedStyle;
use crate::values::{CalcNode, LengthPercentage, Rgba, Size};
use crate::variables::Registry;
use fagus_css::{CascadeBand, Precedence, RuleSet};

/// The settings of one animation of an element: the `animation-*` longhands at its index.
#[derive(Clone, Debug, PartialEq)]
struct Settings {
    name: String,
    /// In seconds.
    duration: f32,
    /// In seconds.
    delay: f32,
    /// `f32::INFINITY` for `infinite`.
    iterations: f32,
    direction: AnimationDirection,
    fill: AnimationFillMode,
    easing: EasingFunction,
}

/// Where an animation is at time zero: the progress of its current iteration (0 to 1, in the
/// direction it plays), or `None` where it has no effect then.
fn progress(settings: &Settings) -> Option<f32> {
    let duration = settings.duration.max(0.0);
    let iterations = settings.iterations.max(0.0);
    let active = if duration == 0.0 || iterations == 0.0 {
        0.0
    } else {
        duration * iterations
    };
    // The local time of the animation at time zero.
    let time = -settings.delay;
    let fills = |modes: [AnimationFillMode; 2]| modes.contains(&settings.fill);
    let (overall, iteration) = if time < 0.0 {
        // Before the delay is over.
        if !fills([AnimationFillMode::Backwards, AnimationFillMode::Both]) {
            return None;
        }
        (0.0, 0.0)
    } else if time >= active {
        // After the animation: where its last iteration ended.
        if !fills([AnimationFillMode::Forwards, AnimationFillMode::Both]) || iterations.is_infinite() {
            return None;
        }
        let fraction = iterations.fract();
        if iterations > 0.0 && fraction == 0.0 {
            (1.0, iterations - 1.0)
        } else {
            (fraction, iterations.floor())
        }
    } else {
        let overall = time / duration;
        (overall.fract(), overall.floor())
    };
    let odd = (iteration % 2.0 - 1.0).abs() < 0.5;
    let reversed = match settings.direction {
        AnimationDirection::Normal => false,
        AnimationDirection::Reverse => true,
        AnimationDirection::Alternate => odd,
        AnimationDirection::AlternateReverse => !odd,
    };
    Some(if reversed { 1.0 - overall } else { overall })
}

/// An easing function applied to a progress between 0 and 1 (CSS Easing 1).
fn ease(easing: &EasingFunction, progress: f32) -> f32 {
    match *easing {
        EasingFunction::Linear => progress,
        EasingFunction::Ease => cubic_bezier((0.25, 0.1, 0.25, 1.0), progress),
        EasingFunction::EaseIn => cubic_bezier((0.42, 0.0, 1.0, 1.0), progress),
        EasingFunction::EaseOut => cubic_bezier((0.0, 0.0, 0.58, 1.0), progress),
        EasingFunction::EaseInOut => cubic_bezier((0.42, 0.0, 0.58, 1.0), progress),
        EasingFunction::CubicBezier { x1, y1, x2, y2 } => cubic_bezier((x1, y1, x2, y2), progress),
        EasingFunction::Steps { count, ref position } => {
            #[allow(clippy::cast_precision_loss, reason = "step counts are small")]
            let steps = count.max(1) as f32;
            let (jumps, start) = match position {
                StepPosition::Start => (steps, 1.0),
                StepPosition::End => (steps, 0.0),
                StepPosition::JumpNone => ((steps - 1.0).max(1.0), 0.0),
                StepPosition::JumpBoth => (steps + 1.0, 1.0),
            };
            let step = ((progress * steps).floor() + start).min(jumps);
            (step / jumps).clamp(0.0, 1.0)
        }
    }
}

/// The output of `cubic-bezier(x1, y1, x2, y2)` at `x`: the curve's parameter for `x` found by
/// Newton's method, with bisection where it does not converge.
fn cubic_bezier((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    if x <= 0.0 || x >= 1.0 {
        return x.clamp(0.0, 1.0);
    }
    let curve = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let slope = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };
    let mut t = x;
    for _ in 0..8 {
        let error = curve(x1, x2, t) - x;
        if error.abs() < 1e-6 {
            return curve(y1, y2, t);
        }
        let d = slope(x1, x2, t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= error / d;
    }
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    t = x;
    for _ in 0..40 {
        let value = curve(x1, x2, t);
        if (value - x).abs() < 1e-6 {
            break;
        }
        if value < x {
            low = t;
        } else {
            high = t;
        }
        t = f32::midpoint(low, high);
    }
    curve(y1, y2, t)
}

/// Whether a declaration sets an animation.
fn is_animation(property: &Property<'static>) -> bool {
    matches!(
        property.property_id(),
        PropertyId::Animation(_) | PropertyId::AnimationName(_)
    )
}

/// The animations of an element from its declarations in cascade order, with `var()`
/// substituted from `style`'s custom properties.
fn settings(
    ordered: &Ordered<'_>,
    expansions: &Expansions,
    style: &ComputedStyle,
    (registry, functions): (&Registry, &Functions<'_>),
) -> Vec<Settings> {
    let declarations = cascade_declarations(ordered, expansions);
    let lookup = |name: &str| {
        style
            .custom_properties
            .get(name)
            .map(AsRef::as_ref)
            .or_else(|| registry.initial(name))
    };
    // The value of a longhand, the later of its declarations with and without `-webkit-`.
    let value = |ids: [PropertyId<'static>; 2]| -> Option<Property<'static>> {
        let (id, declared): (&PropertyId<'static>, &Declared<'_>) = ids
            .iter()
            .filter_map(|id| {
                let position = *declarations.index.get(id)?;
                let (_, declared) = declarations.entries[position].as_ref()?;
                Some((position, (id, declared)))
            })
            .max_by_key(|(position, _)| *position)?
            .1;
        match resolve(declared, id, &lookup, functions)? {
            Ok(property) => Some(property.into_owned()),
            Err(_) => None,
        }
    };
    macro_rules! longhand {
        ($variant:ident) => {
            value([
                PropertyId::$variant(VendorPrefix::None),
                PropertyId::$variant(VendorPrefix::WebKit),
            ])
        };
    }
    let Some(Property::AnimationName(names, _)) = longhand!(AnimationName) else {
        return Vec::new();
    };
    let seconds = |time: &Time| match *time {
        Time::Seconds(seconds) => seconds,
        Time::Milliseconds(milliseconds) => milliseconds / 1000.0,
    };
    let durations: Vec<f32> = match longhand!(AnimationDuration) {
        Some(Property::AnimationDuration(list, _)) => list.iter().map(seconds).collect(),
        _ => Vec::new(),
    };
    let delays: Vec<f32> = match longhand!(AnimationDelay) {
        Some(Property::AnimationDelay(list, _)) => list.iter().map(seconds).collect(),
        _ => Vec::new(),
    };
    let iterations: Vec<f32> = match longhand!(AnimationIterationCount) {
        Some(Property::AnimationIterationCount(list, _)) => list
            .iter()
            .map(|count| match count {
                AnimationIterationCount::Number(number) => *number,
                AnimationIterationCount::Infinite => f32::INFINITY,
            })
            .collect(),
        _ => Vec::new(),
    };
    let directions: Vec<AnimationDirection> = match longhand!(AnimationDirection) {
        Some(Property::AnimationDirection(list, _)) => list.to_vec(),
        _ => Vec::new(),
    };
    let fills: Vec<AnimationFillMode> = match longhand!(AnimationFillMode) {
        Some(Property::AnimationFillMode(list, _)) => list.to_vec(),
        _ => Vec::new(),
    };
    let easings: Vec<EasingFunction> = match longhand!(AnimationTimingFunction) {
        Some(Property::AnimationTimingFunction(list, _)) => list.to_vec(),
        _ => Vec::new(),
    };
    names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let name = match name {
                AnimationName::None => return None,
                AnimationName::Ident(ident) => ident.0.to_string(),
                AnimationName::String(string) => string.0.to_string(),
            };
            Some(Settings {
                name,
                duration: cycle(&durations, index, 0.0),
                delay: cycle(&delays, index, 0.0),
                iterations: cycle(&iterations, index, 1.0),
                direction: cycle(&directions, index, AnimationDirection::Normal),
                fill: cycle(&fills, index, AnimationFillMode::None),
                easing: cycle(&easings, index, EasingFunction::Ease),
            })
        })
        .collect()
}

/// The value at `index` of a list of an `animation-*` longhand, which repeats to as many as
/// there are names (CSS Animations 1, §4.2), or `default` where the list is empty.
fn cycle<T: Clone>(list: &[T], index: usize, default: T) -> T {
    if list.is_empty() {
        default
    } else {
        list[index % list.len()].clone()
    }
}

/// The keyframes that set a property: their offsets, the declaration that sets it and their
/// blocks.
type Frames<'r> = Vec<(f32, &'r Property<'static>, &'r DeclarationBlock<'static>)>;

/// A keyframe: where it lies in an iteration and its declarations.
struct Keyframe<'r> {
    offset: f32,
    block: &'r DeclarationBlock<'static>,
}

/// Whether a declaration may be animated from a keyframe: not the animation's own settings,
/// nor its `animation-timing-function`, which gives the keyframe's easing (CSS Animations 1,
/// §3), nor the properties whose animation type is "not animatable", which keyframes leave
/// alone (CSS Contain 2 for `contain`, WPT `contain-animation-001`).
fn animatable(property: &Property<'static>) -> bool {
    let name = property.property_id().name().to_ascii_lowercase();
    !name.starts_with("animation")
        && !name.starts_with("-webkit-animation")
        && !name.starts_with("transition")
        && !matches!(
            name.as_str(),
            "contain"
                | "container"
                | "container-type"
                | "container-name"
                | "will-change"
                | "direction"
                | "unicode-bidi"
                | "writing-mode"
                | "-webkit-writing-mode"
                | "text-orientation"
        )
}

/// The easing of the keyframe whose block is `block`, if it sets one.
fn keyframe_easing(block: &DeclarationBlock<'static>) -> Option<EasingFunction> {
    block
        .declarations
        .iter()
        .rev()
        .find_map(|property| match property {
            Property::AnimationTimingFunction(list, _) => list.first().cloned(),
            _ => None,
        })
}

/// One property's place in an animation at time zero: the declarations of the keyframes
/// before and after the progress (`None` where it is the element's own value), and the eased
/// progress between them.
struct Step<'r> {
    id: PropertyId<'static>,
    from: Option<&'r Property<'static>>,
    to: Option<&'r Property<'static>>,
    progress: f32,
}

/// The steps of the animation with `settings` whose keyframes are `rule`'s.
fn steps<'r>(
    rule: &'r lightningcss::rules::keyframes::KeyframesRule<'static>,
    settings: &Settings,
) -> Vec<Step<'r>> {
    let Some(progress) = progress(settings) else {
        return Vec::new();
    };
    let mut keyframes: Vec<Keyframe<'r>> = rule
        .keyframes
        .iter()
        .flat_map(|keyframe| {
            keyframe.selectors.iter().filter_map(move |selector| {
                let offset = match selector {
                    KeyframeSelector::From => 0.0,
                    KeyframeSelector::To => 1.0,
                    KeyframeSelector::Percentage(percentage) => percentage.0,
                    KeyframeSelector::TimelineRangePercentage(_) => return None,
                };
                (0.0..=1.0).contains(&offset).then_some(Keyframe {
                    offset,
                    block: &keyframe.declarations,
                })
            })
        })
        .collect();
    keyframes.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    // Every property any keyframe sets, with the declaration of each keyframe that sets it
    // (the later of a keyframe's declarations of it).
    let mut properties: Vec<(PropertyId<'static>, Frames<'r>)> = Vec::new();
    for keyframe in &keyframes {
        for property in &keyframe.block.declarations {
            if !animatable(property) {
                continue;
            }
            for longhand in expand(property) {
                let id = cascade_id(&longhand);
                let index = properties
                    .iter()
                    .position(|(known, _)| *known == id)
                    .unwrap_or_else(|| {
                        properties.push((id, Vec::new()));
                        properties.len() - 1
                    });
                match properties[index].1.last_mut() {
                    Some(last)
                        if last.0.total_cmp(&keyframe.offset).is_eq()
                            && std::ptr::eq(last.2, keyframe.block) =>
                    {
                        last.1 = property;
                    }
                    _ => properties[index]
                        .1
                        .push((keyframe.offset, property, keyframe.block)),
                }
            }
        }
    }
    properties
        .into_iter()
        .map(|(id, frames)| {
            // The keyframe at or before the progress, and the one after it; the element's own
            // value stands at 0% and 100% where no keyframe sets the property there.
            let before = frames.iter().rev().find(|frame| frame.0 <= progress);
            let after = frames.iter().find(|frame| frame.0 > progress);
            let start = before.map_or(0.0, |frame| frame.0);
            let end = after.map_or(1.0, |frame| frame.0);
            let local = if end > start {
                ((progress - start) / (end - start)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let easing = before
                .and_then(|frame| keyframe_easing(frame.2))
                .unwrap_or_else(|| settings.easing.clone());
            let (from, to) = (before.map(|frame| frame.1), after.map(|frame| frame.1));
            // At a keyframe the value is the keyframe's, unless the easing jumps at the start
            // of the interval (`steps(n, start)`) while the animation runs (CSS Easing 1, §3.1:
            // before its delay is over, the "before flag" keeps the step from jumping).
            let progress = if before.is_some_and(|frame| frame.0.total_cmp(&progress).is_eq()) {
                if after.is_some() && settings.delay <= 0.0 {
                    ease(&easing, 0.0)
                } else {
                    0.0
                }
            } else {
                ease(&easing, local)
            };
            Step {
                id,
                from,
                to,
                progress,
            }
        })
        .collect()
}

/// The precedence of declarations from keyframes: above every normal declaration, below the
/// important ones.
const ANIMATED: Precedence = Precedence {
    band: CascadeBand::InlineNormal,
    layer: u32::MAX,
    specificity: u32::MAX,
    proximity: u32::MAX,
    order: u32::MAX,
};

/// Gives `style`, built from `ordered` by `build`, the values its animations have at time
/// zero.
pub(super) fn animate(
    style: &mut ComputedStyle,
    rules: &RuleSet,
    ordered: &Ordered<'_>,
    (expansions, registry, functions): (&Expansions, &Registry, &Functions<'_>),
    build: &dyn Fn(&Ordered<'_>) -> ComputedStyle,
) {
    if !rules.has_keyframes() || !ordered.iter().any(|(_, property)| is_animation(property)) {
        return;
    }
    let animations = settings(ordered, expansions, style, (registry, functions));
    let all: Vec<Step<'_>> = animations
        .iter()
        .filter_map(|settings| Some(steps(rules.keyframes(&settings.name)?, settings)))
        .flatten()
        .collect();
    if all.is_empty() {
        return;
    }
    let with = |side: &dyn for<'s> Fn(&'s Step<'_>) -> Option<&'s Property<'static>>| {
        let mut declarations = ordered.to_vec();
        declarations.extend(all.iter().filter_map(side).map(|property| (ANIMATED, property)));
        declarations.sort_by_key(|(precedence, _)| *precedence);
        build(&declarations)
    };
    // Each property takes the nearer keyframe's value; where it lies between two, colors and
    // opacity are interpolated.
    let mut animated = with(&|step| if step.progress < 0.5 { step.from } else { step.to });
    let between = all
        .iter()
        .filter(|step| step.progress > 0.0 && step.progress < 1.0 && interpolates(&step.id))
        .collect::<Vec<_>>();
    if !between.is_empty() {
        let from = with(&|step| step.from);
        let to = with(&|step| step.to);
        for step in between {
            interpolate(&step.id, (&from, &to), step.progress, &mut animated);
        }
    }
    *style = animated;
}

/// Whether the values of a property are interpolated between keyframes (rather than flipping
/// at the middle).
fn interpolates(id: &PropertyId<'static>) -> bool {
    matches!(
        id,
        PropertyId::Color
            | PropertyId::BackgroundColor
            | PropertyId::Opacity
            | PropertyId::BorderTopColor
            | PropertyId::BorderRightColor
            | PropertyId::BorderBottomColor
            | PropertyId::BorderLeftColor
            | PropertyId::OutlineColor
            | PropertyId::BoxShadow(_)
            | PropertyId::Width
            | PropertyId::Height
            | PropertyId::Filter(_)
            | PropertyId::Transform(_)
            | PropertyId::Translate
            | PropertyId::Rotate
            | PropertyId::Scale
    )
}

/// Two colors mixed as Chrome interpolates them: premultiplied by their alpha, in sRGB.
pub(super) fn mix(from: Rgba, to: Rgba, progress: f32) -> Rgba {
    let a = from.a + (to.a - from.a) * progress;
    if a <= 0.0 {
        return Rgba::TRANSPARENT;
    }
    let channel = |x: f32, y: f32| (x * from.a + (y * to.a - x * from.a) * progress) / a;
    Rgba {
        r: channel(from.r, to.r),
        g: channel(from.g, to.g),
        b: channel(from.b, to.b),
        a,
    }
}

/// Sets the property `id` of `style` to its value `progress` of the way between `from`'s and
/// `to`'s.
fn interpolate(
    id: &PropertyId<'static>,
    (from, to): (&ComputedStyle, &ComputedStyle),
    progress: f32,
    style: &mut ComputedStyle,
) {
    // A side without a color of its own has the element's (`currentcolor`).
    let mix_side = |a: Option<Rgba>, b: Option<Rgba>| {
        Some(mix(a.unwrap_or(from.color), b.unwrap_or(to.color), progress))
    };
    match id {
        PropertyId::Color => style.color = mix(from.color, to.color, progress),
        PropertyId::BackgroundColor => {
            style.background_color = mix(from.background_color, to.background_color, progress);
        }
        PropertyId::Opacity => style.opacity = from.opacity + (to.opacity - from.opacity) * progress,
        PropertyId::BorderTopColor => {
            style.border_color.top = mix_side(from.border_color.top, to.border_color.top);
        }
        PropertyId::BorderRightColor => {
            style.border_color.right = mix_side(from.border_color.right, to.border_color.right);
        }
        PropertyId::BorderBottomColor => {
            style.border_color.bottom = mix_side(from.border_color.bottom, to.border_color.bottom);
        }
        PropertyId::BorderLeftColor => {
            style.border_color.left = mix_side(from.border_color.left, to.border_color.left);
        }
        PropertyId::OutlineColor => {
            style.outline_color = mix_side(from.outline_color, to.outline_color);
        }
        PropertyId::BoxShadow(_) => {
            if let Some(shadows) = mix_shadows((from, to), progress) {
                style.box_shadow = shadows;
            }
        }
        PropertyId::Width => {
            if let Some(width) = mix_sizes(&from.width, &to.width, progress) {
                style.width = width;
            }
        }
        PropertyId::Height => {
            if let Some(height) = mix_sizes(&from.height, &to.height, progress) {
                style.height = height;
            }
        }
        PropertyId::Filter(_) => {
            if let Some(filter) =
                super::interpolate::filters(&from.filter, &to.filter, progress, (from.color, to.color))
            {
                style.filter = filter;
            }
        }
        PropertyId::Transform(_) => {
            if let Some(transform) = super::interpolate::transforms(&from.transform, &to.transform, progress)
            {
                style.transform = transform;
            }
        }
        PropertyId::Translate => {
            style.translate =
                super::interpolate::translate(from.translate.as_ref(), to.translate.as_ref(), progress);
        }
        PropertyId::Rotate => {
            if let Some(rotate) = super::interpolate::rotate(from.rotate, to.rotate, progress) {
                style.rotate = rotate;
            }
        }
        PropertyId::Scale => style.scale = super::interpolate::scale(from.scale, to.scale, progress),
        _ => {}
    }
}

/// Two sizes interpolated, where both are lengths or percentages (CSS Values 4, §3.1.1): a
/// length or percentage where both are of a kind, else their mix as a calculation; `None`
/// for keywords, whose values flip at the middle.
fn mix_sizes(from: &Size, to: &Size, progress: f32) -> Option<Size> {
    let (Size::Length(a), Size::Length(b)) = (from, to) else {
        return None;
    };
    Some(Size::Length(mix_lengths(a, b, progress)))
}

/// Two lengths or percentages interpolated: a length or percentage where both are of a kind,
/// else their mix as a calculation.
pub(super) fn mix_lengths(a: &LengthPercentage, b: &LengthPercentage, progress: f32) -> LengthPercentage {
    let lerp = |a: f32, b: f32| a + (b - a) * progress;
    match (a, b) {
        (LengthPercentage::Px(a), LengthPercentage::Px(b)) => LengthPercentage::Px(lerp(*a, *b)),
        (LengthPercentage::Percent(a), LengthPercentage::Percent(b)) => {
            LengthPercentage::Percent(lerp(*a, *b))
        }
        _ => {
            let node = |value: &LengthPercentage| match value {
                LengthPercentage::Px(px) => CalcNode::Px(*px),
                LengthPercentage::Percent(fraction) => CalcNode::Percent(*fraction),
                LengthPercentage::Calc(node) => (**node).clone(),
            };
            LengthPercentage::Calc(std::sync::Arc::new(CalcNode::Sum(
                Box::new(CalcNode::Product(1.0 - progress, Box::new(node(a)))),
                Box::new(CalcNode::Product(progress, Box::new(node(b)))),
            )))
        }
    }
}

/// Two lists of box shadows interpolated (CSS Backgrounds 3, §7.2): the shorter padded with
/// transparent shadows of no size, each pair's offsets, blur, spread and color mixed; `None`
/// where a pair is one inset and one outer shadow, whose values flip at the middle.
fn mix_shadows(
    (from, to): (&ComputedStyle, &ComputedStyle),
    progress: f32,
) -> Option<std::sync::Arc<[crate::values::BoxShadow]>> {
    use crate::values::BoxShadow;
    let none = |inset| BoxShadow {
        color: Some(Rgba::TRANSPARENT),
        x: 0.0,
        y: 0.0,
        blur: 0.0,
        spread: 0.0,
        inset,
    };
    let length = from.box_shadow.len().max(to.box_shadow.len());
    let lerp = |a: f32, b: f32| a + (b - a) * progress;
    (0..length)
        .map(|index| {
            let (a, b) = match (from.box_shadow.get(index), to.box_shadow.get(index)) {
                (Some(a), Some(b)) => (a.clone(), b.clone()),
                (Some(a), None) => (a.clone(), none(a.inset)),
                (None, Some(b)) => (none(b.inset), b.clone()),
                (None, None) => return None,
            };
            (a.inset == b.inset).then(|| BoxShadow {
                // A shadow without a color of its own has the element's (`currentcolor`).
                color: Some(mix(
                    a.color.unwrap_or(from.color),
                    b.color.unwrap_or(to.color),
                    progress,
                )),
                x: lerp(a.x, b.x),
                y: lerp(a.y, b.y),
                blur: lerp(a.blur, b.blur),
                spread: lerp(a.spread, b.spread),
                inset: a.inset,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(duration: f32, delay: f32) -> Settings {
        Settings {
            name: "a".into(),
            duration,
            delay,
            iterations: 1.0,
            direction: AnimationDirection::Normal,
            fill: AnimationFillMode::None,
            easing: EasingFunction::Ease,
        }
    }

    #[test]
    fn finds_the_progress_at_time_zero() {
        assert_eq!(progress(&settings(2.0, 0.0)), Some(0.0));
        assert_eq!(progress(&settings(10.0, -5.0)), Some(0.5));
        // Not started yet: only a backwards fill shows the first keyframe.
        assert_eq!(progress(&settings(1.0, 1.0)), None);
        let backwards = Settings {
            fill: AnimationFillMode::Both,
            ..settings(1.0, 1.0)
        };
        assert_eq!(progress(&backwards), Some(0.0));
        // Over already: only a forwards fill keeps the last.
        assert_eq!(progress(&settings(1.0, -2.0)), None);
        let forwards = Settings {
            fill: AnimationFillMode::Forwards,
            ..settings(1.0, -2.0)
        };
        assert_eq!(progress(&forwards), Some(1.0));
        // A zero duration is over at once.
        assert_eq!(progress(&settings(0.0, 0.0)), None);
        // The second iteration of an alternating animation runs backwards.
        let alternate = Settings {
            iterations: f32::INFINITY,
            direction: AnimationDirection::Alternate,
            ..settings(4.0, -5.0)
        };
        assert_eq!(progress(&alternate), Some(0.75));
        let reverse = Settings {
            direction: AnimationDirection::Reverse,
            ..settings(2.0, 0.0)
        };
        assert_eq!(progress(&reverse), Some(1.0));
    }

    #[test]
    fn eases_progress() {
        assert!((cubic_bezier((0.0, 1.0, 1.0, 0.0), 0.5) - 0.5).abs() < 1e-4);
        assert!((ease(&EasingFunction::EaseIn, 0.5) - 0.315).abs() < 0.01);
        assert!(ease(&EasingFunction::EaseOut, 0.5) > 0.6);
        let steps = |count, position| EasingFunction::Steps { count, position };
        assert_eq!(ease(&steps(4, StepPosition::End), 0.3), 0.25);
        assert_eq!(ease(&steps(4, StepPosition::Start), 0.3), 0.5);
        assert_eq!(ease(&steps(2, StepPosition::End), 0.0), 0.0);
    }

    #[test]
    fn mixes_premultiplied_colors() {
        let green = Rgba::from_u8(0, 200, 0, 1.0);
        let red = Rgba::from_u8(200, 0, 0, 1.0);
        let middle = mix(green, red, 0.5);
        assert!((middle.r - 100.0 / 255.0).abs() < 1e-5 && (middle.g - 100.0 / 255.0).abs() < 1e-5);
        // A transparent end keeps the other's color.
        let half = mix(Rgba::TRANSPARENT, red, 0.5);
        assert!((half.r - 200.0 / 255.0).abs() < 1e-5 && (half.a - 0.5).abs() < 1e-5);
    }
}
