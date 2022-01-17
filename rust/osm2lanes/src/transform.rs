use std::iter;

use serde::{Deserialize, Serialize};

use crate::tags::{TagKey, Tags, TagsRead, TagsWrite};
use crate::{BufferType, Config, Direction, DrivingSide, LaneSpec, LaneType, Lanes};

const HIGHWAY: TagKey = TagKey::from("highway");
const CYCLEWAY: TagKey = TagKey::from("cycleway");

#[derive(Clone, Debug, PartialEq)]
enum WaySide {
    Both,
    Right,
    Left,
}

impl WaySide {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Right => "right",
            Self::Left => "left",
        }
    }
}

impl ToString for WaySide {
    fn to_string(&self) -> String {
        self.as_str().to_owned()
    }
}

impl std::convert::From<DrivingSide> for WaySide {
    fn from(side: DrivingSide) -> Self {
        match side {
            DrivingSide::Right => Self::Right,
            DrivingSide::Left => Self::Left,
        }
    }
}

impl std::convert::From<DrivingSide> for TagKey {
    fn from(side: DrivingSide) -> Self {
        match side {
            DrivingSide::Right => Self::from("right"),
            DrivingSide::Left => Self::from("left"),
        }
    }
}

impl DrivingSide {
    fn tag(&self) -> TagKey {
        (*self).into()
    }
}

impl LaneSpec {
    fn forward(lane_type: LaneType) -> Self {
        Self {
            lane_type,
            direction: Direction::Forward,
        }
    }
    fn backward(lane_type: LaneType) -> Self {
        Self {
            lane_type,
            direction: Direction::Backward,
        }
    }
    fn both(lane_type: LaneType) -> Self {
        Self {
            lane_type,
            direction: Direction::Both,
        }
    }
    fn _none(lane_type: LaneType) -> Self {
        Self {
            lane_type,
            direction: Direction::None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaneSpecError(String);

impl ToString for LaneSpecError {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LaneSpecWarnings(Vec<LaneSpecWarning>);

impl LaneSpecWarnings {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl ToString for LaneSpecWarnings {
    fn to_string(&self) -> String {
        self.0
            .iter()
            .map(|warn| format!("Warning: {}", warn.description))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaneSpecWarning {
    pub description: String,
    // Tags relevant to triggering the warning
    // TODO: investigate making this a view of keys on a Tags object instead
    pub tags: Tags,
}

type LaneSpecResult = Result<Lanes, LaneSpecError>;

// Handle non motorized ways
fn non_motorized(tags: &Tags, cfg: &Config) -> Option<LaneSpecResult> {
    if !tags.is_any(
        HIGHWAY,
        &[
            "cycleway",
            "footway",
            "path",
            "pedestrian",
            "steps",
            "track",
        ],
    ) {
        log::trace!("motorized");
        return None;
    }
    // Easy special cases first.
    if tags.is(HIGHWAY, "steps") {
        return Some(Ok(Lanes {
            lanes: vec![LaneSpec::both(LaneType::Sidewalk)],
            warnings: LaneSpecWarnings(vec![LaneSpecWarning {
                description: "highway is steps, but lane is only a sidewalk".to_owned(),
                tags: tags.subset(&[HIGHWAY]),
            }]),
        }));
    }

    // Eventually, we should have some kind of special LaneType for shared walking/cycling paths of
    // different kinds. Until then, model by making bike lanes and a shoulder for walking.

    // If it just allows foot traffic, simply make it a sidewalk. For most of the above highway
    // types, assume bikes are allowed, except for footways, where they must be explicitly
    // allowed.
    if tags.is("bicycle", "no")
        || (tags.is(HIGHWAY, "footway") && !tags.is_any("bicycle", &["designated", "yes"]))
    {
        return Some(Ok(Lanes {
            lanes: vec![LaneSpec::both(LaneType::Sidewalk)],
            warnings: LaneSpecWarnings::default(),
        }));
    }
    // Otherwise, there'll always be a bike lane.

    let mut forward_side = vec![LaneSpec::forward(LaneType::Biking)];
    let mut backward_side = if tags.is("oneway", "yes") {
        vec![]
    } else {
        vec![LaneSpec::backward(LaneType::Biking)]
    };

    if !tags.is("foot", "no") {
        forward_side.push(LaneSpec::both(LaneType::Shoulder));
        if !backward_side.is_empty() {
            backward_side.push(LaneSpec::both(LaneType::Shoulder));
        }
    }
    Some(Ok(Lanes {
        lanes: assemble_ltr(forward_side, backward_side, cfg.driving_side),
        warnings: LaneSpecWarnings::default(),
    }))
}

fn driving_lane_directions(tags: &Tags, _cfg: &Config, oneway: bool) -> (usize, usize) {
    let both_ways = if let Some(n) = tags
        .get("lanes:both_ways")
        .and_then(|num| num.parse::<usize>().ok())
    {
        n
    } else {
        0
    };
    let num_driving_fwd = if let Some(n) = tags
        .get("lanes:forward")
        .and_then(|num| num.parse::<usize>().ok())
    {
        n
    } else if let Some(n) = tags.get("lanes").and_then(|num| num.parse::<usize>().ok()) {
        let half = if oneway {
            n
        } else {
            // usize division rounded up
            (n + 1) / 2
        };
        half - both_ways
    } else {
        1
    };
    let num_driving_back = if let Some(n) = tags
        .get("lanes:backward")
        .and_then(|num| num.parse::<usize>().ok())
    {
        n
    } else if let Some(n) = tags.get("lanes").and_then(|num| num.parse::<usize>().ok()) {
        let base = n - num_driving_fwd;
        let half = if oneway {
            base
        } else {
            // lanes=1 but not oneway... what is this supposed to mean?
            base.max(1)
        };
        half - both_ways
    } else if oneway {
        0
    } else {
        1
    };
    (num_driving_fwd, num_driving_back)
}

fn bus(
    tags: &Tags,
    _cfg: &Config,
    oneway: bool,
    forward_side: &mut [LaneSpec],
    backward_side: &mut [LaneSpec],
) {
    let fwd_bus_spec = if let Some(s) = tags.get("bus:lanes:forward") {
        s
    } else if let Some(s) = tags.get("psv:lanes:forward") {
        s
    } else if oneway {
        if let Some(s) = tags.get("bus:lanes") {
            s
        } else if let Some(s) = tags.get("psv:lanes") {
            s
        } else {
            ""
        }
    } else {
        ""
    };
    if !fwd_bus_spec.is_empty() {
        let parts: Vec<&str> = fwd_bus_spec.split('|').collect();
        let offset = if forward_side[0].lane_type == LaneType::SharedLeftTurn {
            1
        } else {
            0
        };
        if parts.len() == forward_side.len() - offset {
            for (idx, part) in parts.into_iter().enumerate() {
                if part == "designated" {
                    forward_side[idx + offset].lane_type = LaneType::Bus;
                }
            }
        }
    }
    if let Some(spec) = tags
        .get("bus:lanes:backward")
        .or_else(|| tags.get("psv:lanes:backward"))
    {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == backward_side.len() {
            for (idx, part) in parts.into_iter().enumerate() {
                if part == "designated" {
                    backward_side[idx].lane_type = LaneType::Bus;
                }
            }
        }
    }
}

fn bicycle(
    tags: &Tags,
    cfg: &Config,
    oneway: bool,
    forward_side: &mut Vec<LaneSpec>,
    backward_side: &mut Vec<LaneSpec>,
    warnings: &mut LaneSpecWarnings,
) -> Result<(), LaneSpecError> {
    impl Tags {
        fn is_cycleway(&self, side: Option<WaySide>) -> bool {
            if let Some(side) = side {
                self.is_any(CYCLEWAY + side.as_str(), &["lane", "track"])
            } else {
                self.is_any(CYCLEWAY, &["lane", "track"])
            }
        }
    }

    if tags.is_cycleway(None) {
        if tags.is_cycleway(Some(WaySide::Both))
            || tags.is_cycleway(Some(WaySide::Right))
            || tags.is_cycleway(Some(WaySide::Left))
        {
            return Err(LaneSpecError(
                "cycleway=* not supported with any cycleway:* values".to_owned(),
            ));
        }
        forward_side.push(LaneSpec::forward(LaneType::Biking));
        if oneway {
            if !backward_side.is_empty() {
                // TODO safety check to be checked
                warnings.0.push(LaneSpecWarning {
                    description: "oneway has backwards lanes when adding cycleways".to_owned(),
                    tags: tags.subset(&["oneway", "cycleway"]),
                })
            }
        } else {
            backward_side.push(LaneSpec::backward(LaneType::Biking));
        }
    } else if tags.is_cycleway(Some(WaySide::Both)) {
        forward_side.push(LaneSpec::both(LaneType::Biking));
    } else {
        // cycleway=opposite_lane
        if tags.is(CYCLEWAY, "opposite_lane") {
            warnings.0.push(LaneSpecWarning {
                description: "cycleway=opposite_lane deprecated".to_owned(),
                tags: tags.subset(&[CYCLEWAY]),
            });
            backward_side.push(LaneSpec::backward(LaneType::Biking));
        }
        // cycleway:FORWARD=*
        if tags.is_cycleway(Some(cfg.driving_side.into())) {
            if tags.is(CYCLEWAY + cfg.driving_side.tag() + "oneway", "no")
                || tags.is("oneway:bicycle", "no")
            {
                forward_side.push(LaneSpec::both(LaneType::Biking));
            } else {
                forward_side.push(LaneSpec::forward(LaneType::Biking));
            }
        }
        // cycleway:FORWARD=opposite_lane
        if tags.is_any(
            CYCLEWAY + cfg.driving_side.tag(),
            &["opposite_lane", "opposite_track"],
        ) {
            warnings.0.push(LaneSpecWarning {
                description: "cycleway:FORWARD=opposite_lane deprecated".to_owned(),
                tags: tags.subset(&[CYCLEWAY]), // TODO make side specific
            });
            forward_side.push(LaneSpec::backward(LaneType::Biking));
        }
        // cycleway:BACKWARD=*
        if tags.is_cycleway(Some(cfg.driving_side.opposite().into())) {
            if tags.is(
                CYCLEWAY + cfg.driving_side.opposite().tag() + "oneway",
                "no",
            ) || tags.is("oneway:bicycle", "no")
            {
                backward_side.push(LaneSpec::both(LaneType::Biking));
            } else if oneway {
                // A oneway road with a cycleway on the wrong side
                forward_side.insert(0, LaneSpec::forward(LaneType::Biking));
            } else {
                // A contraflow bicycle lane
                backward_side.push(LaneSpec::backward(LaneType::Biking));
            }
        }
        // cycleway:BACKWARD=opposite_lane
        if tags.is_any(
            CYCLEWAY + cfg.driving_side.opposite().tag(),
            &["opposite_lane", "opposite_track"],
        ) {
            return Err(LaneSpecError(
                "cycleway:BACKWARD=opposite_lane unsupported".to_owned(),
            ));
        }
    }

    // My brain hurts. How does the above combinatorial explosion play with
    // https://wiki.openstreetmap.org/wiki/Proposed_features/cycleway:separation? Let's take the
    // "post-processing" approach.

    // TODO Not attempting left-handed driving yet.
    if cfg.driving_side == DrivingSide::Left
        && forward_side
            .iter()
            .chain(backward_side.iter())
            .any(|lane| lane.lane_type == LaneType::Biking)
    {
        return Err(LaneSpecError("LHT with cycleways not supported".to_owned()));
    }

    // TODO A two-way cycletrack on one side of a one-way road will almost definitely break this.
    if let Some(buffer) = tags
        .get("cycleway:right:separation:left")
        .and_then(osm_separation_type)
    {
        // TODO These shouldn't fail, but snapping is imperfect... like around
        // https://www.openstreetmap.org/way/486283205
        if let Some(idx) = forward_side
            .iter()
            .position(|x| x.lane_type == LaneType::Biking)
        {
            forward_side.insert(idx, LaneSpec::forward(LaneType::Buffer(buffer)));
        }
    }
    if let Some(buffer) = tags
        .get("cycleway:left:separation:left")
        .and_then(osm_separation_type)
    {
        if let Some(idx) = backward_side
            .iter()
            .position(|x| x.lane_type == LaneType::Biking)
        {
            backward_side.insert(idx, LaneSpec::backward(LaneType::Buffer(buffer)));
        }
    }
    if let Some(buffer) = tags
        .get("cycleway:left:separation:right")
        .and_then(osm_separation_type)
    {
        // This is assuming a one-way road. That's why we're not looking at back_side.
        if let Some(idx) = forward_side
            .iter()
            .position(|x| x.lane_type == LaneType::Biking)
        {
            forward_side.insert(idx + 1, LaneSpec::forward(LaneType::Buffer(buffer)));
        }
    }

    Ok(())
}

fn parking(
    tags: &Tags,
    _cfg: &Config,
    _oneway: bool,
    forward_side: &mut Vec<LaneSpec>,
    backward_side: &mut Vec<LaneSpec>,
) {
    let has_parking = vec!["parallel", "diagonal", "perpendicular"];
    let parking_lane_fwd = tags.is_any("parking:lane:right", &has_parking)
        || tags.is_any("parking:lane:both", &has_parking);
    let parking_lane_back = tags.is_any("parking:lane:left", &has_parking)
        || tags.is_any("parking:lane:both", &has_parking);
    if parking_lane_fwd {
        forward_side.push(LaneSpec::forward(LaneType::Parking));
    }
    if parking_lane_back {
        backward_side.push(LaneSpec::backward(LaneType::Parking));
    }
}

fn walking(
    tags: &Tags,
    cfg: &Config,
    _oneway: bool,
    forward_side: &mut Vec<LaneSpec>,
    backward_side: &mut Vec<LaneSpec>,
) {
    if tags.is("sidewalk", "both") {
        forward_side.push(LaneSpec::both(LaneType::Sidewalk));
        backward_side.push(LaneSpec::both(LaneType::Sidewalk));
    } else if tags.is("sidewalk", "separate") && cfg.inferred_sidewalks {
        // TODO Need to snap separate sidewalks to ways. Until then, just do this.
        forward_side.push(LaneSpec::forward(LaneType::Sidewalk));
        if !backward_side.is_empty() {
            backward_side.push(LaneSpec::both(LaneType::Sidewalk));
        }
    } else if tags.is("sidewalk", "right") {
        if cfg.driving_side == DrivingSide::Right {
            forward_side.push(LaneSpec::both(LaneType::Sidewalk));
        } else {
            backward_side.push(LaneSpec::both(LaneType::Sidewalk));
        }
    } else if tags.is("sidewalk", "left") {
        if cfg.driving_side == DrivingSide::Right {
            backward_side.push(LaneSpec::both(LaneType::Sidewalk));
        } else {
            forward_side.push(LaneSpec::both(LaneType::Sidewalk));
        }
    }

    let mut need_fwd_shoulder = forward_side
        .last()
        .map(|spec| spec.lane_type != LaneType::Sidewalk)
        .unwrap_or(true);
    let mut need_back_shoulder = backward_side
        .last()
        .map(|spec| spec.lane_type != LaneType::Sidewalk)
        .unwrap_or(true);
    if tags.is_any(HIGHWAY, &["motorway", "motorway_link", "construction"])
        || tags.is("foot", "no")
        || tags.is("access", "no")
        || tags.is("motorroad", "yes")
    {
        need_fwd_shoulder = false;
        need_back_shoulder = false;
    }
    // If it's a one-way, fine to not have sidewalks on both sides.
    if tags.is("oneway", "yes") {
        need_back_shoulder = false;
    }

    // For living streets in Krakow, there aren't separate footways. People can walk in the street.
    // For now, model that by putting shoulders.
    if cfg.inferred_sidewalks || tags.is(HIGHWAY, "living_street") {
        if need_fwd_shoulder {
            forward_side.push(LaneSpec::both(LaneType::Shoulder));
        }
        if need_back_shoulder {
            backward_side.push(LaneSpec::both(LaneType::Shoulder));
        }
    }
}

/// From an OpenStreetMap way's tags, determine the lanes along the road from left to right.
pub fn get_lane_specs_ltr_with_warnings(tags: &Tags, cfg: &Config) -> LaneSpecResult {
    let mut warnings = LaneSpecWarnings::default();

    if let Some(spec) = non_motorized(tags, cfg) {
        return spec;
    }

    // TODO Reversible roads should be handled differently?
    let oneway = tags.is_any("oneway", &["yes", "reversible"]) || tags.is("junction", "roundabout");

    let (num_driving_fwd, num_driving_back) = driving_lane_directions(tags, cfg, oneway);

    let driving_lane = if tags.is("access", "no")
        && (tags.is("bus", "yes") || tags.is("psv", "yes")) // West Seattle
        || tags
            .get("motor_vehicle:conditional")
            .map(|x| x.starts_with("no"))
            .unwrap_or(false)
            && tags.is("bus", "yes")
    // Example: 3rd Ave in downtown Seattle
    {
        LaneType::Bus
    } else if tags.is("access", "no") || tags.is("highway", "construction") {
        LaneType::Construction
    } else {
        LaneType::Driving
    };

    // These are ordered from the road center, going outwards. Most of the members of fwd_side will
    // have Direction::Forward, but there can be exceptions with two-way cycletracks.
    let mut fwd_side: Vec<LaneSpec> = iter::repeat_with(|| LaneSpec::forward(driving_lane))
        .take(num_driving_fwd)
        .collect();
    let mut back_side: Vec<LaneSpec> = iter::repeat_with(|| LaneSpec::backward(driving_lane))
        .take(num_driving_back)
        .collect();
    // TODO Fix upstream. https://wiki.openstreetmap.org/wiki/Key:centre_turn_lane
    if tags.is("lanes:both_ways", "1") || tags.is("centre_turn_lane", "yes") {
        fwd_side.insert(0, LaneSpec::both(LaneType::SharedLeftTurn));
    }

    if driving_lane == LaneType::Construction {
        return Ok(Lanes {
            lanes: assemble_ltr(fwd_side, back_side, cfg.driving_side),
            warnings: LaneSpecWarnings::default(),
        });
    }

    bus(tags, cfg, oneway, &mut fwd_side, &mut back_side);

    bicycle(
        tags,
        cfg,
        oneway,
        &mut fwd_side,
        &mut back_side,
        &mut warnings,
    )?;

    if driving_lane == LaneType::Driving {
        parking(tags, cfg, oneway, &mut fwd_side, &mut back_side);
    }

    walking(tags, cfg, oneway, &mut fwd_side, &mut back_side);

    Ok(Lanes {
        lanes: assemble_ltr(fwd_side, back_side, cfg.driving_side),
        warnings,
    })
}

pub fn get_lane_specs_ltr(tags: &Tags, cfg: &Config) -> Result<Vec<LaneSpec>, LaneSpecError> {
    let Lanes {
        lanes: lane_specs,
        warnings,
    } = get_lane_specs_ltr_with_warnings(tags, cfg)?;
    if !warnings.0.is_empty() {
        return Err(LaneSpecError(format!(
            "{} warnings found",
            warnings.0.len()
        )));
    }
    Ok(lane_specs)
}

fn assemble_ltr(
    mut fwd_side: Vec<LaneSpec>,
    mut back_side: Vec<LaneSpec>,
    driving_side: DrivingSide,
) -> Vec<LaneSpec> {
    match driving_side {
        DrivingSide::Right => {
            back_side.reverse();
            back_side.extend(fwd_side);
            back_side
        }
        DrivingSide::Left => {
            fwd_side.reverse();
            fwd_side.extend(back_side);
            fwd_side
        }
    }
}

// See https://wiki.openstreetmap.org/wiki/Proposed_features/cycleway:separation#Typical_values.
// Lots of these mappings are pretty wacky right now. We need more BufferTypes.
fn osm_separation_type(x: &str) -> Option<BufferType> {
    match x {
        "bollard" | "vertical_panel" => Some(BufferType::FlexPosts),
        "kerb" | "separation_kerb" => Some(BufferType::Curb),
        "grass_verge" | "planter" | "tree_row" => Some(BufferType::Planters),
        "guard_rail" | "jersey_barrier" | "railing" => Some(BufferType::JerseyBarrier),
        // TODO Make sure there's a parking lane on that side... also mapped? Any flex posts in
        // between?
        "parking_lane" => None,
        "barred_area" | "dashed_line" | "solid_line" => Some(BufferType::Stripes),
        _ => None,
    }
}

pub fn lanes_to_tags(lanes: &[LaneSpec], _cfg: &Config) -> Result<Tags, LaneSpecError> {
    let mut tags = Tags::default();
    let mut oneway = false;
    tags.insert("highway", "yes"); // TODO, what?
    {
        let lane_count = lanes
            .iter()
            .filter(|lane| {
                lane.lane_type == LaneType::Driving
                    || lane.lane_type == LaneType::SharedLeftTurn
                    || lane.lane_type == LaneType::Bus
            })
            .count();
        tags.insert("lanes", lane_count.to_string());
    }
    // Oneway
    if lanes
        .iter()
        .filter(|lane| lane.lane_type == LaneType::Driving)
        .all(|lane| lane.direction == Direction::Forward)
    {
        tags.insert("oneway", "yes");
        oneway = true;
    }
    // Pedestrian
    {
        match (
            lanes.first().unwrap().lane_type == LaneType::Sidewalk,
            lanes.last().unwrap().lane_type == LaneType::Sidewalk,
        ) {
            (false, false) => {}
            (true, false) => assert!(tags.insert("sidewalk", "left").is_none()),
            (false, true) => assert!(tags.insert("sidewalk", "right").is_none()),
            (true, true) => assert!(tags.insert("sidewalk", "both").is_none()),
        }
    }
    // Parking
    match (
        lanes
            .iter()
            .take_while(|lane| lane.lane_type != LaneType::Driving)
            .find(|lane| lane.lane_type == LaneType::Parking)
            .is_some(),
        lanes
            .iter()
            .skip_while(|lane| lane.lane_type != LaneType::Driving)
            .find(|lane| lane.lane_type == LaneType::Parking)
            .is_some(),
    ) {
        (false, false) => {}
        (true, false) => assert!(tags.insert("parking:lane:left", "parallel").is_none()),
        (false, true) => assert!(tags.insert("parking:lane:right", "parallel").is_none()),
        (true, true) => assert!(tags.insert("parking:lane:both", "parallel").is_none()),
    }
    // Cycleway
    {
        let left_cycle_lane = lanes
            .iter()
            .take_while(|lane| lane.lane_type != LaneType::Driving)
            .find(|lane| lane.lane_type == LaneType::Biking);
        let right_cycle_lane = lanes
            .iter()
            .rev()
            .take_while(|lane| lane.lane_type != LaneType::Driving)
            .find(|lane| lane.lane_type == LaneType::Biking);
        match (left_cycle_lane.is_some(), right_cycle_lane.is_some()) {
            (false, false) => {}
            (true, false) => assert!(tags.insert("cycleway:left", "lane").is_none()),
            (false, true) => assert!(tags.insert("cycleway:right", "lane").is_none()),
            (true, true) => assert!(tags.insert("cycleway:both", "lane").is_none()),
        }
        // https://wiki.openstreetmap.org/wiki/Key:cycleway:right:oneway
        // TODO, incomplete, pending testing.
        if !oneway {
            if let Some(LaneSpec {
                lane_type: _,
                direction: Direction::Both,
            }) = left_cycle_lane
            {
                tags.insert("cycleway:left:oneway", "no");
            }
            if let Some(LaneSpec {
                lane_type: _,
                direction: Direction::Both,
            }) = right_cycle_lane
            {
                tags.insert("cycleway:right:oneway", "no");
            }
        } else {
            if let Some(LaneSpec {
                lane_type: _,
                direction: Direction::Both,
            }) = left_cycle_lane
            {
                tags.insert("cycleway:left:oneway", "no");
            }
            if let Some(LaneSpec {
                lane_type: _,
                direction: Direction::Both,
            }) = right_cycle_lane
            {
                tags.insert("cycleway:right:oneway", "no");
            }
        }
    }
    if lanes
        .iter()
        .find(|lane| lane.lane_type == LaneType::SharedLeftTurn)
        .is_some()
    {
        tags.insert("lanes:both_ways", "1");
        // TODO: add LHT support
        tags.insert("turn:lanes:both_ways", "left");
    }
    Ok(tags)
}