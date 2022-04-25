#![allow(clippy::module_name_repetitions)] // TODO: fix upstream

use crate::locale::Locale;
use crate::road::Road;
use crate::tag::{TagKey, Tags};
use crate::transform::error::{RoadError, RoadWarnings};
use crate::transform::RoadFromTags;

mod error;
pub use error::TagsToLanesMsg;

mod access_by_lane;

mod lane;

mod modes;

mod separator;

mod road;
use road::{LaneBuilder, LaneBuilderError, LaneType, RoadBuilder};

#[non_exhaustive]
pub struct Config {
    pub error_on_warnings: bool,
    pub include_separators: bool,
}

impl Config {
    #[must_use]
    pub fn new(error_on_warnings: bool, include_separators: bool) -> Self {
        Self {
            error_on_warnings,
            include_separators,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            error_on_warnings: false,
            include_separators: true,
        }
    }
}

mod oneway {
    #[derive(Clone, Copy, PartialEq)]
    pub enum Oneway {
        Yes,
        No,
    }

    impl std::convert::From<bool> for Oneway {
        fn from(oneway: bool) -> Self {
            if oneway {
                Oneway::Yes
            } else {
                Oneway::No
            }
        }
    }

    impl std::convert::From<Oneway> for bool {
        fn from(oneway: Oneway) -> Self {
            match oneway {
                Oneway::Yes => true,
                Oneway::No => false,
            }
        }
    }
}
use oneway::Oneway;

mod infer {
    // TODO: implement try when this is closed: https://github.com/rust-lang/rust/issues/84277
    /// A value with various levels of inference
    #[derive(Copy, Clone, Debug)]
    pub enum Infer<T> {
        None,
        Default(T),
        Calculated(T),
        Direct(T),
    }

    impl<T> Infer<T> {
        pub fn some(self) -> Option<T> {
            match self {
                Self::None => None,
                Self::Default(v) | Self::Calculated(v) | Self::Direct(v) => Some(v),
            }
        }

        pub(super) fn direct(some: Option<T>) -> Self {
            match some {
                None => Self::None,
                Some(v) => Self::Direct(v),
            }
        }

        pub fn map<U, F>(self, f: F) -> Infer<U>
        where
            F: FnOnce(T) -> U,
        {
            match self {
                Infer::None => Infer::None,
                Infer::Default(x) => Infer::Default(f(x)),
                Infer::Calculated(x) => Infer::Calculated(f(x)),
                Infer::Direct(x) => Infer::Direct(f(x)),
            }
        }
    }

    impl<T> Default for Infer<T> {
        fn default() -> Self {
            Self::None
        }
    }
}
use infer::Infer;

/// From an OpenStreetMap way's tags,
/// determine the lanes along the road from left to right.
///
/// # Errors
///
/// Warnings or errors are produced for situations that may make the lanes inaccurate, such as:
///
/// - Unimplemented or sunuspported tags
/// - Ambiguous tags
/// - Unknown internal errors
///
/// If the issue may be recoverable, a warning is preferred.
/// A config option allows all warnings to be treated as errors.
///
pub fn tags_to_lanes(
    tags: &Tags,
    locale: &Locale,
    config: &Config,
) -> Result<RoadFromTags, RoadError> {
    let mut warnings = RoadWarnings::default();

    // Early return if we find unimplemented tags.
    unsupported(tags, locale, &mut warnings)?;

    // Create the road builder and start giving it schemes.
    let mut road: RoadBuilder = RoadBuilder::from(tags, locale, &mut warnings)?;

    // Early return for non-motorized ways (pedestrian paths, cycle paths, etc.)
    if let Some(spec) = modes::non_motorized(tags, locale, &road)? {
        return Ok(spec);
    }

    modes::bus(tags, locale, &mut road, &mut warnings)?;

    modes::bicycle(tags, locale, &mut road, &mut warnings)?;

    modes::parking(tags, locale, &mut road)?;

    modes::foot_and_shoulder(tags, locale, &mut road, &mut warnings)?;

    let (lanes, highway, _oneway) =
        road.into_ltr(tags, locale, config.include_separators, &mut warnings)?;

    let road_from_tags = RoadFromTags {
        road: Road { lanes, highway },
        warnings,
    };

    if config.error_on_warnings && !road_from_tags.warnings.is_empty() {
        return Err(road_from_tags.warnings.into());
    }

    Ok(road_from_tags)
}

/// Unsupported
///
/// # Errors
///
/// Oneway reversible
pub fn unsupported(
    tags: &Tags,
    _locale: &Locale,
    warnings: &mut RoadWarnings,
) -> Result<(), RoadError> {
    // https://wiki.openstreetmap.org/wiki/Key:access#Transport_mode_restrictions
    const ACCESS_KEYS: [&str; 43] = [
        "access",
        "dog",
        "ski",
        "inline_skates",
        "horse",
        "vehicle",
        "bicycle",
        "electric_bicycle",
        "carriage",
        "hand_cart",
        "quadracycle",
        "trailer",
        "caravan",
        "motor_vehicle",
        "motorcycle",
        "moped",
        "mofa",
        "motorcar",
        "motorhome",
        "tourist_bus",
        "coach",
        "goods",
        "hgv",
        "hgv_articulated",
        "bdouble",
        "agricultural",
        "golf_cart",
        "atv",
        "snowmobile",
        "psv",
        "bus",
        "taxi",
        "minibus",
        "share_taxi",
        "hov",
        "car_sharing",
        "emergency",
        "hazmat",
        "disabled",
        "roadtrain",
        "hgv_caravan",
        "lhv",
        "tank",
    ];
    if ACCESS_KEYS
        .iter()
        .any(|k| tags.get(TagKey::from(k)).is_some())
    {
        warnings.push(TagsToLanesMsg::unimplemented(
            "access",
            // TODO, TagTree should support subset
            tags.subset(&ACCESS_KEYS),
        ));
    }

    if tags.is("oneway", "reversible") {
        // TODO reversible roads should be handled differently
        return Err(TagsToLanesMsg::unimplemented_tag("oneway", "reversible").into());
    }

    Ok(())
}
