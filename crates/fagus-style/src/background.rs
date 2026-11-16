//! Computed values of backgrounds: images, gradients and the layer lists (CSS Backgrounds 3,
//! CSS Images 3 and 4).

use std::sync::Arc;

use fagus_css::gradient::{InterpolationMethod, repair};
use lightningcss::properties::background::{
    BackgroundClip, BackgroundOrigin, BackgroundRepeat as CssRepeat, BackgroundRepeatKeyword,
    BackgroundSize as CssSize,
};
use lightningcss::properties::custom::{CustomProperty, CustomPropertyName, Token, TokenList, TokenOrValue};
use lightningcss::properties::{Property, PropertyId};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions};
use lightningcss::traits::{IntoOwned, Parse};
use lightningcss::values::angle::Angle;
use lightningcss::values::color::{CssColor, FloatColor};
use lightningcss::values::gradient::{
    Circle, ColorStop, Ellipse, EndingShape, Gradient as CssGradient, GradientItem, LineDirection,
    ShapeExtent,
};
use lightningcss::values::image::Image as CssImage;
use lightningcss::values::length::LengthPercentage as CssLengthPercentage;
use lightningcss::values::percentage::DimensionPercentage;
use lightningcss::values::position::{
    HorizontalPositionKeyword, Position as CssPosition, PositionComponent, VerticalPositionKeyword,
};

use crate::compute::{Context, color};
use crate::values::{
    BackgroundRepeat, BackgroundSize, BoxArea, CalcNode, ColorSpace, Gradient, GradientKind, GradientStop,
    HueInterpolation, Image, Interpolation, LengthPercentage, LengthPercentageAuto, LinearDirection,
    RadialShape, RadialSize, StopPosition,
};

/// The name under which `background-image` values with interpolation methods are kept as tokens.
pub const IMAGE_TOKENS: &str = "background-image";

pub fn image(value: &CssImage<'_>, context: &Context, method: Option<&InterpolationMethod>) -> Image {
    match value {
        CssImage::None => Image::None,
        CssImage::Url(url) => Image::Url(Arc::from(url.url.as_ref()), 1.0),
        CssImage::Gradient(gradient) => gradient_value(gradient, context, method)
            .map_or(Image::None, |gradient| Image::Gradient(Arc::new(gradient))),
        CssImage::ImageSet(set) => {
            image_set_candidate(&set.options).map_or(Image::None, |(option, resolution)| {
                match image(&option.image, context, method) {
                    Image::Url(url, _) => Image::Url(url, resolution),
                    other => other,
                }
            })
        }
    }
}

/// The candidate of `image-set()` that Chrome prints, with its resolution in image pixels per
/// CSS pixel: of the types it shows, the one of the lowest resolution not below 1x, or else of
/// the highest.
fn image_set_candidate<'a>(
    options: &'a [lightningcss::values::image::ImageSetOption<'_>],
) -> Option<(&'a lightningcss::values::image::ImageSetOption<'a>, f32)> {
    use lightningcss::values::resolution::Resolution;
    let shown = |option: &&lightningcss::values::image::ImageSetOption<'_>| {
        option.file_type.as_ref().is_none_or(|kind| {
            matches!(
                kind.to_ascii_lowercase().as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/svg+xml"
            )
        })
    };
    let candidates = options.iter().filter(shown).filter_map(|option| {
        let resolution = match option.resolution {
            Resolution::Dppx(dppx) => dppx,
            Resolution::Dpi(dpi) => dpi / 96.0,
            Resolution::Dpcm(dpcm) => dpcm * 2.54 / 96.0,
        };
        (resolution.is_finite() && resolution > 0.0).then_some((option, resolution))
    });
    let (sharp, blurred): (Vec<_>, Vec<_>) = candidates.partition(|(_, resolution)| *resolution >= 1.0);
    sharp
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .or_else(|| blurred.into_iter().rev().max_by(|a, b| a.1.total_cmp(&b.1)))
}

fn gradient_value(
    gradient: &CssGradient,
    context: &Context,
    method: Option<&InterpolationMethod>,
) -> Option<Gradient> {
    let (kind, repeating, mut stops): (_, _, Vec<GradientStop>) = match gradient {
        CssGradient::Linear(linear) | CssGradient::RepeatingLinear(linear) => (
            GradientKind::Linear(direction(&linear.direction)),
            matches!(gradient, CssGradient::RepeatingLinear(_)),
            linear
                .items
                .iter()
                .map(|item| length_stop(item, context))
                .collect(),
        ),
        CssGradient::Radial(radial) | CssGradient::RepeatingRadial(radial) => (
            GradientKind::Radial {
                shape: shape(&radial.shape, context),
                position: position(&radial.position, context),
            },
            matches!(gradient, CssGradient::RepeatingRadial(_)),
            radial
                .items
                .iter()
                .map(|item| length_stop(item, context))
                .collect(),
        ),
        CssGradient::Conic(conic) | CssGradient::RepeatingConic(conic) => (
            GradientKind::Conic {
                // An infinite or undefined angle (`calc(infinity * 1deg)`) as no turn (Chrome
                // 157, measured).
                from: Some(conic.angle.to_degrees())
                    .filter(|angle| angle.is_finite())
                    .unwrap_or(0.0),
                position: position(&conic.position, context),
            },
            matches!(gradient, CssGradient::RepeatingConic(_)),
            conic.items.iter().map(angle_stop).collect(),
        ),
        // `-webkit-gradient()` from before the standard.
        CssGradient::WebKitGradient(_) => return None,
    };
    let interpolation = method.and_then(interpolation);
    split_missing(gradient, &mut stops, interpolation);
    Some(Gradient {
        kind,
        repeating,
        stops,
        interpolation,
    })
}

/// Gives the stops whose colors miss components (`none`) the colors they stand for on each
/// side (see [`crate::missing_components`]).
fn split_missing(gradient: &CssGradient, stops: &mut [GradientStop], interpolation: Option<Interpolation>) {
    fn colors<D>(items: &[GradientItem<D>]) -> Vec<Option<&CssColor>> {
        items
            .iter()
            .filter_map(|item| match item {
                GradientItem::ColorStop(stop) => {
                    Some(Some(&stop.color).filter(|color| **color != CssColor::CurrentColor))
                }
                GradientItem::Hint(_) => None,
            })
            .collect()
    }
    let colors = match gradient {
        CssGradient::Linear(linear) | CssGradient::RepeatingLinear(linear) => colors(&linear.items),
        CssGradient::Radial(radial) | CssGradient::RepeatingRadial(radial) => colors(&radial.items),
        CssGradient::Conic(conic) | CssGradient::RepeatingConic(conic) => colors(&conic.items),
        CssGradient::WebKitGradient(_) => return,
    };
    // Without a method, sRGB for legacy colors and Oklab for others (CSS Images 4, §3.1).
    let all_legacy = stops
        .iter()
        .all(|stop| !matches!(stop, GradientStop::Color { legacy: false, .. }));
    let space = interpolation.map_or(
        if all_legacy {
            ColorSpace::Srgb
        } else {
            ColorSpace::Oklab
        },
        |method| method.space,
    );
    let split = crate::missing_components::split(&colors, space);
    let color_stops = stops.iter_mut().filter_map(|stop| match stop {
        GradientStop::Color { color, after, .. } => Some((color, after)),
        GradientStop::Hint(_) => None,
    });
    for ((color, after), split) in color_stops.zip(split) {
        if let Some((before, beyond)) = split {
            *color = Some(before);
            *after = (beyond != before).then_some(beyond);
        }
    }
}

fn interpolation(method: &InterpolationMethod) -> Option<Interpolation> {
    Some(Interpolation {
        space: ColorSpace::from_name(&method.space)?,
        hue: match method.hue.as_deref() {
            Some("longer") => HueInterpolation::Longer,
            Some("increasing") => HueInterpolation::Increasing,
            Some("decreasing") => HueInterpolation::Decreasing,
            _ => HueInterpolation::Shorter,
        },
    })
}

fn direction(direction: &LineDirection) -> LinearDirection {
    match direction {
        LineDirection::Angle(angle) => LinearDirection::Angle(angle.to_degrees()),
        LineDirection::Horizontal(HorizontalPositionKeyword::Left) => LinearDirection::Angle(270.0),
        LineDirection::Horizontal(HorizontalPositionKeyword::Right) => LinearDirection::Angle(90.0),
        LineDirection::Vertical(VerticalPositionKeyword::Top) => LinearDirection::Angle(0.0),
        LineDirection::Vertical(VerticalPositionKeyword::Bottom) => LinearDirection::Angle(180.0),
        LineDirection::Corner { horizontal, vertical } => LinearDirection::Corner {
            right: *horizontal == HorizontalPositionKeyword::Right,
            bottom: *vertical == VerticalPositionKeyword::Bottom,
        },
    }
}

fn shape(shape: &EndingShape, context: &Context) -> RadialShape {
    let extent = |extent: &ShapeExtent| match extent {
        ShapeExtent::ClosestSide => RadialSize::ClosestSide,
        ShapeExtent::FarthestSide => RadialSize::FarthestSide,
        ShapeExtent::ClosestCorner => RadialSize::ClosestCorner,
        ShapeExtent::FarthestCorner => RadialSize::FarthestCorner,
    };
    match shape {
        EndingShape::Circle(Circle::Radius(radius)) => {
            let radius = LengthPercentage::Px(context.length(radius));
            RadialShape::Circle(RadialSize::Radii(radius.clone(), radius))
        }
        EndingShape::Circle(Circle::Extent(size)) => RadialShape::Circle(extent(size)),
        EndingShape::Ellipse(Ellipse::Size { x, y }) => RadialShape::Ellipse(RadialSize::Radii(
            context.length_percentage(x),
            context.length_percentage(y),
        )),
        EndingShape::Ellipse(Ellipse::Extent(size)) => RadialShape::Ellipse(extent(size)),
    }
}

/// A position as offsets from the top left corner (`right 10px` becomes `calc(100% - 10px)`).
pub fn position(position: &CssPosition, context: &Context) -> (LengthPercentage, LengthPercentage) {
    (
        component(&position.x, context, |side| {
            *side == HorizontalPositionKeyword::Right
        }),
        component(&position.y, context, |side| {
            *side == VerticalPositionKeyword::Bottom
        }),
    )
}

pub fn component<S>(
    component: &PositionComponent<S>,
    context: &Context,
    is_far_side: impl Fn(&S) -> bool,
) -> LengthPercentage {
    match component {
        PositionComponent::Center => LengthPercentage::Percent(0.5),
        PositionComponent::Length(length) => context.length_percentage(length),
        PositionComponent::Side { side, offset } => {
            let offset = offset
                .as_ref()
                .map_or(LengthPercentage::ZERO, |offset| context.length_percentage(offset));
            if is_far_side(side) {
                LengthPercentage::from_node(CalcNode::Sum(
                    Box::new(CalcNode::Percent(1.0)),
                    Box::new(CalcNode::Product(-1.0, Box::new(offset.to_node()))),
                ))
            } else {
                offset
            }
        }
    }
}

fn is_legacy(color: &CssColor) -> bool {
    match color {
        CssColor::RGBA(_) | CssColor::CurrentColor | CssColor::System(_) => true,
        CssColor::Float(float) => matches!(**float, FloatColor::HSL(_) | FloatColor::HWB(_)),
        CssColor::LightDark(light, _) => is_legacy(light),
        _ => false,
    }
}

fn color_stop<D>(stop: &ColorStop<D>, position: Option<StopPosition>) -> GradientStop {
    GradientStop::Color {
        color: color(&stop.color),
        legacy: is_legacy(&stop.color),
        position,
        after: None,
    }
}

fn length_stop(item: &GradientItem<CssLengthPercentage>, context: &Context) -> GradientStop {
    let length = |value: &CssLengthPercentage| StopPosition::Length(context.length_percentage(value));
    match item {
        GradientItem::ColorStop(stop) => color_stop(stop, stop.position.as_ref().map(length)),
        GradientItem::Hint(hint) => GradientStop::Hint(length(hint)),
    }
}

fn angle_stop(item: &GradientItem<DimensionPercentage<Angle>>) -> GradientStop {
    let position = |value: &DimensionPercentage<Angle>| match value {
        DimensionPercentage::Dimension(angle) => StopPosition::Angle(angle.to_degrees()),
        DimensionPercentage::Percentage(percentage) => {
            StopPosition::Length(LengthPercentage::Percent(percentage.0))
        }
        // An angle with a percentage in calc(): taken as its angle part.
        DimensionPercentage::Calc(_) => StopPosition::Angle(0.0),
    };
    match item {
        GradientItem::ColorStop(stop) => color_stop(stop, stop.position.as_ref().map(position)),
        GradientItem::Hint(hint) => GradientStop::Hint(position(hint)),
    }
}

pub fn size(size: &CssSize, context: &Context) -> BackgroundSize {
    match size {
        CssSize::Explicit { width, height } => BackgroundSize::Explicit(
            context.length_percentage_auto(width),
            context.length_percentage_auto(height),
        ),
        CssSize::Cover => BackgroundSize::Cover,
        CssSize::Contain => BackgroundSize::Contain,
    }
}

pub fn repeat(repeat: &CssRepeat) -> (BackgroundRepeat, BackgroundRepeat) {
    let keyword = |keyword: &BackgroundRepeatKeyword| match keyword {
        BackgroundRepeatKeyword::Repeat => BackgroundRepeat::Repeat,
        BackgroundRepeatKeyword::Space => BackgroundRepeat::Space,
        BackgroundRepeatKeyword::Round => BackgroundRepeat::Round,
        BackgroundRepeatKeyword::NoRepeat => BackgroundRepeat::NoRepeat,
    };
    (keyword(&repeat.x), keyword(&repeat.y))
}

pub fn origin(origin: &BackgroundOrigin) -> BoxArea {
    match origin {
        BackgroundOrigin::BorderBox => BoxArea::BorderBox,
        BackgroundOrigin::PaddingBox => BoxArea::PaddingBox,
        BackgroundOrigin::ContentBox => BoxArea::ContentBox,
    }
}

pub fn clip(clip: &BackgroundClip) -> BoxArea {
    match clip {
        BackgroundClip::BorderBox | BackgroundClip::Border | BackgroundClip::Text => BoxArea::BorderBox,
        BackgroundClip::PaddingBox => BoxArea::PaddingBox,
        BackgroundClip::ContentBox => BoxArea::ContentBox,
    }
}

/// Splits tokens at top-level commas.
fn layers<'t>(tokens: &'t [TokenOrValue<'static>]) -> Vec<&'t [TokenOrValue<'static>]> {
    tokens
        .split(|token| matches!(token, TokenOrValue::Token(Token::Comma)))
        .collect()
}

pub(crate) fn css(tokens: &[TokenOrValue<'static>]) -> Option<String> {
    Property::Custom(CustomProperty {
        name: CustomPropertyName::Custom(lightningcss::values::ident::DashedIdent("--fagus".into())),
        value: TokenList(tokens.iter().map(with_missing_components).collect()),
    })
    .value_to_css_string(PrinterOptions::default())
    .ok()
}

/// A token with its `rgb()`, `hsl()` and `hwb()` colors that miss components (`none`) as
/// functions, which keep them where lightningcss would write the colors as hex.
fn with_missing_components(token: &TokenOrValue<'static>) -> TokenOrValue<'static> {
    match token {
        TokenOrValue::Color(CssColor::Float(float)) => {
            let (name, components) = match **float {
                FloatColor::RGB(c) => ("rgb", [c.r, c.g, c.b, c.alpha]),
                FloatColor::HSL(c) => ("hsl", [c.h, c.s, c.l, c.alpha]),
                FloatColor::HWB(c) => ("hwb", [c.h, c.w, c.b, c.alpha]),
            };
            if !components.iter().any(|component| component.is_nan()) {
                return token.clone();
            }
            let mut arguments = Vec::with_capacity(8);
            for (index, component) in components.into_iter().enumerate() {
                if index == 3 {
                    arguments.push(TokenOrValue::Token(Token::Delim('/')));
                }
                arguments.push(TokenOrValue::Token(if component.is_nan() {
                    Token::Ident("none".into())
                } else {
                    Token::Number {
                        has_sign: component < 0.0,
                        value: component,
                        int_value: None,
                    }
                }));
                arguments.push(TokenOrValue::Token(Token::WhiteSpace(" ".into())));
            }
            arguments.pop();
            TokenOrValue::Function(lightningcss::properties::custom::Function {
                name: lightningcss::values::ident::Ident(name.into()),
                arguments: TokenList(arguments),
            })
        }
        TokenOrValue::Function(function) => {
            TokenOrValue::Function(lightningcss::properties::custom::Function {
                name: function.name.clone(),
                arguments: TokenList(function.arguments.0.iter().map(with_missing_components).collect()),
            })
        }
        other => other.clone(),
    }
}

/// `background-image` from tokens that needed a repair: each layer is parsed by lightningcss
/// after [`repair`], and its interpolation method is added back.
pub fn images_from_tokens(tokens: &TokenList<'static>, context: &Context) -> Option<Vec<Image>> {
    layers(&tokens.0)
        .into_iter()
        .map(|layer| {
            let (stripped, methods) = repair(&TokenList(layer.to_vec()));
            let css = css(&stripped.0).filter(|css| !fagus_css::nests_too_deep(css))?;
            let parsed = CssImage::parse_string(css.trim()).ok()?;
            Some(image(&parsed, context, methods.first().and_then(Option::as_ref)))
        })
        .collect()
}

/// The longhands of a `background` or `background-image` declaration with gradients that need a
/// [`repair`]: lightningcss parses the repaired declaration, and the image layers are kept as
/// tokens under [`IMAGE_TOKENS`] for [`images_from_tokens`].
pub fn repaired_longhands(
    id: &PropertyId<'static>,
    tokens: &TokenList<'static>,
) -> Option<Vec<Property<'static>>> {
    let images = match id {
        PropertyId::BackgroundImage => tokens.clone(),
        PropertyId::Background => {
            // The image of each layer: a gradient, `url()` or nothing.
            let mut images: Vec<TokenOrValue<'static>> = Vec::new();
            for (index, layer) in layers(&tokens.0).into_iter().enumerate() {
                if index > 0 {
                    images.push(TokenOrValue::Token(Token::Comma));
                }
                let image = layer.iter().find(|token| match token {
                    TokenOrValue::Function(function) => {
                        function.name.0.to_ascii_lowercase().ends_with("gradient")
                    }
                    TokenOrValue::Url(_) => true,
                    _ => false,
                });
                images.push(match image {
                    Some(image) => image.clone(),
                    None => TokenOrValue::Token(Token::Ident("none".into())),
                });
            }
            TokenList(images)
        }
        _ => return None,
    };
    let (stripped, _) = repair(tokens);
    let css = css(&stripped.0).filter(|css| !fagus_css::nests_too_deep(css))?;
    let parsed = Property::parse_string(id.clone(), &css, ParserOptions::default())
        .ok()?
        .into_owned();
    let image_tokens = Property::Custom(CustomProperty {
        name: CustomPropertyName::Unknown(lightningcss::values::ident::Ident(IMAGE_TOKENS.into())),
        value: images,
    });
    Some(
        crate::cascade::expand(&parsed)
            .into_iter()
            .map(|longhand| {
                if longhand.property_id() == PropertyId::BackgroundImage {
                    image_tokens.clone()
                } else {
                    longhand
                }
            })
            .collect(),
    )
}

impl LengthPercentage {
    /// The value as a `calc()` node.
    pub fn to_node(&self) -> CalcNode {
        match self {
            Self::Px(px) => CalcNode::Px(*px),
            Self::Percent(fraction) => CalcNode::Percent(*fraction),
            Self::Calc(node) => (**node).clone(),
        }
    }
}

impl LengthPercentageAuto {
    pub fn length(&self) -> Option<&LengthPercentage> {
        match self {
            Self::Auto => None,
            Self::Length(length) => Some(length),
        }
    }
}
