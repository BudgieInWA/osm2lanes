use serde::{Deserialize, Serialize};

use super::Markings;
use crate::locale::Locale;
use crate::metric::{Metre, Speed};
use crate::tag::{Access as AccessValue, HighwayType};

/// A single lane
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Lane {
    Travel {
        // TODO, we could make this non-optional, but remove the field for designated=foot?
        #[serde(skip_serializing_if = "Option::is_none")]
        direction: Option<Direction>,
        designated: Designated,
        #[serde(skip_serializing_if = "Option::is_none")]
        width: Option<Metre>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_speed: Option<Speed>,
        #[serde(skip_serializing_if = "Option::is_none")]
        access: Option<Access>,
    },
    Parking {
        direction: Direction,
        designated: Designated,
        #[serde(skip_serializing_if = "Option::is_none")]
        width: Option<Metre>,
    },
    Shoulder {
        #[serde(skip_serializing_if = "Option::is_none")]
        width: Option<Metre>,
    },
    Separator {
        markings: Markings,
    },
}

impl Lane {
    // EUROPEAN AGREEMENT 1 ON MAIN INTERNATIONAL TRAFFIC ARTERIES (AGR) 1975
    // III.1.1.1
    pub const DEFAULT_WIDTH: Metre = Metre::new(3.5);

    /// Width in metres
    #[must_use]
    pub fn width(&self, locale: &Locale, highway: HighwayType) -> Metre {
        match self {
            Lane::Separator { markings } => markings.width(locale),
            Lane::Travel {
                width, designated, ..
            } => width.unwrap_or_else(|| locale.travel_width(designated, highway)),
            // TODO: parking different from travel?
            Lane::Parking {
                width, designated, ..
            } => width.unwrap_or_else(|| locale.travel_width(designated, highway)),
            Lane::Shoulder { width, .. } => width.unwrap_or(Lane::DEFAULT_WIDTH),
        }
    }

    /// Mirror the lane
    #[must_use]
    pub fn mirror(self) -> Self {
        match self {
            Self::Separator { mut markings } => {
                markings.flip();
                Self::Separator { markings }
            },
            _ => self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Forward,
    Backward,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Designated {
    // #[serde(rename = "any")]
    // Any,
    #[serde(rename = "foot")]
    Foot,
    #[serde(rename = "bicycle")]
    Bicycle,
    #[serde(rename = "motor_vehicle")]
    Motor,
    #[serde(rename = "bus")]
    Bus,
}

/// Display lane detail as printable characters
pub trait Printable {
    fn as_ascii(&self) -> char;
    fn as_utf8(&self) -> char;
}

impl Printable for Lane {
    fn as_ascii(&self) -> char {
        match self {
            Self::Travel {
                designated: Designated::Foot,
                ..
            } => 's',
            Self::Travel {
                designated: Designated::Bicycle,
                ..
            } => 'b',
            Self::Travel {
                designated: Designated::Motor,
                ..
            } => 'd',
            Self::Travel {
                designated: Designated::Bus,
                ..
            } => 'B',
            Self::Shoulder { .. } => 'S',
            Self::Parking { .. } => 'p',
            Self::Separator { .. } => '|',
        }
    }
    fn as_utf8(&self) -> char {
        match self {
            Self::Travel {
                designated: Designated::Foot,
                ..
            } => '🚶',
            Self::Travel {
                designated: Designated::Bicycle,
                ..
            } => '🚲',
            Self::Travel {
                designated: Designated::Motor,
                ..
            } => '🚗',
            Self::Travel {
                designated: Designated::Bus,
                ..
            } => '🚌',
            Self::Shoulder { .. } => '🛆',
            Self::Parking { .. } => '🅿',
            Self::Separator { .. } => '|',
        }
    }
}

impl Printable for Direction {
    fn as_ascii(&self) -> char {
        match self {
            Self::Forward => '^',
            Self::Backward => 'v',
            Self::Both => '|',
        }
    }
    fn as_utf8(&self) -> char {
        match self {
            Self::Forward => '↑',
            Self::Backward => '↓',
            Self::Both => '↕',
        }
    }
}

/// Access by vehicle type
/// Types as defined in <https://wiki.openstreetmap.org/wiki/Key:access#Land-based_transportation>
// TODO: how to handle the motor_vehicle vs motorcar discussion in https://wiki.openstreetmap.org/wiki/Key:motorcar#Controversy
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub struct Access {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) foot: Option<AccessValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bicycle: Option<AccessValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) taxi: Option<AccessValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bus: Option<AccessValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) motor: Option<AccessValue>,
}
