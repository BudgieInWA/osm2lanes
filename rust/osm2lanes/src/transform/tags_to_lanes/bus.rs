use super::*;

const LANES: TagKey = TagKey::from("lanes");

impl RoadError {
    fn unsupported_str(description: &str) -> Self {
        RoadMsg::unsupported_str(description).into()
    }
}

impl LaneBuilder {
    fn set_bus(&mut self, _locale: &Locale) -> Result<(), LaneBuilderError> {
        self.designated = Infer::Direct(LaneDesignated::Bus);
        Ok(())
    }
}

pub(super) fn bus(
    tags: &Tags,
    locale: &Locale,
    oneway: Oneway,
    forward_side: &mut [LaneBuilder],
    backward_side: &mut [LaneBuilder],
    warnings: &mut RoadWarnings,
) -> ModeResult {
    // https://wiki.openstreetmap.org/wiki/Bus_lanes
    // 3 schemes, for simplicity we only allow one at a time
    match (
        tags.tree().get("busway").is_some(),
        tags.tree()
            .get("lanes:bus")
            .or_else(|| tags.tree().get("lanes:psv"))
            .is_some(),
        tags.tree()
            .get("bus:lanes")
            .or_else(|| tags.tree().get("psv:lanes"))
            .is_some(),
    ) {
        (false, false, false) => {}
        (true, _, false) => busway(tags, locale, oneway, forward_side, backward_side, warnings)?,
        (false, true, false) => {
            lanes_bus(tags, locale, oneway, forward_side, backward_side, warnings)?
        }
        (false, false, true) => {
            bus_lanes(tags, locale, oneway, forward_side, backward_side, warnings)?
        }
        _ => {
            return Err(RoadMsg::Unsupported {
                description: Some("more than one bus lanes scheme used".to_owned()),
                tags: None,
            }
            .into())
        }
    }

    Ok(())
}

fn busway(
    tags: &Tags,
    locale: &Locale,
    _oneway: Oneway,
    forward_side: &mut [LaneBuilder],
    backward_side: &mut [LaneBuilder],
    _warnings: &mut RoadWarnings,
) -> Result<(), RoadError> {
    const BUSWAY: TagKey = TagKey::from("busway");
    if tags.is(BUSWAY, "lane") {
        forward_side
            .last_mut()
            .ok_or_else(|| RoadError::unsupported_str("no forward lanes for busway"))?
            .set_bus(locale)?;
        if !tags.is("oneway", "yes") && !tags.is("oneway:bus", "yes") {
            backward_side
                .last_mut()
                .ok_or_else(|| RoadError::unsupported_str("no backward lanes for busway"))?
                .set_bus(locale)?;
        }
    }
    if tags.is(BUSWAY, "opposite_lane") {
        backward_side
            .last_mut()
            .ok_or_else(|| RoadError::unsupported_str("no backward lanes for busway"))?
            .set_bus(locale)?;
    }
    if tags.is(BUSWAY + "both", "lane") {
        forward_side
            .last_mut()
            .ok_or_else(|| RoadError::unsupported_str("no forward lanes for busway"))?
            .set_bus(locale)?;
        backward_side
            .last_mut()
            .ok_or_else(|| RoadError::unsupported_str("no backward lanes for busway"))?
            .set_bus(locale)?;
        if tags.is("oneway", "yes") || tags.is("oneway:bus", "yes") {
            return Err(RoadError::ambiguous_str(
                "busway:both=lane for oneway roads",
            ));
        }
    }
    if tags.is(BUSWAY + locale.driving_side.tag(), "lane") {
        forward_side
            .last_mut()
            .ok_or_else(|| RoadError::unsupported_str("no forward lanes for busway"))?
            .set_bus(locale)?;
    }
    if tags.is(BUSWAY + locale.driving_side.opposite().tag(), "lane") {
        if tags.is("oneway", "yes") || tags.is("oneway:bus", "yes") {
            forward_side
                .first_mut()
                .ok_or_else(|| RoadError::unsupported_str("no forward lanes for busway"))?
                .set_bus(locale)?;
        } else {
            return Err(RoadError::ambiguous_str(
                "busway:BACKWARD=lane for bidirectional roads",
            ));
        }
    }
    Ok(())
}

fn lanes_bus(
    tags: &Tags,
    _locale: &Locale,
    _oneway: Oneway,
    _forward_side: &mut [LaneBuilder],
    _backward_side: &mut [LaneBuilder],
    warnings: &mut RoadWarnings,
) -> ModeResult {
    warnings.push(RoadMsg::Unimplemented {
        description: None,
        tags: Some(tags.subset(&[
            LANES + "psv",
            LANES + "psv" + "forward",
            LANES + "psv" + "backward",
            LANES + "psv" + "left",
            LANES + "psv" + "right",
            LANES + "bus",
            LANES + "bus" + "forward",
            LANES + "bus" + "backward",
            LANES + "bus" + "left",
            LANES + "bus" + "right",
        ])),
    });
    Ok(())
}

fn bus_lanes(
    tags: &Tags,
    locale: &Locale,
    oneway: Oneway,
    forward_side: &mut [LaneBuilder],
    backward_side: &mut [LaneBuilder],
    _warnings: &mut RoadWarnings,
) -> ModeResult {
    let fwd_bus_spec = if let Some(s) = tags.get("bus:lanes:forward") {
        s
    } else if let Some(s) = tags.get("psv:lanes:forward") {
        s
    } else if oneway.into() {
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
        let offset = if forward_side[0].direction.some() == Some(LaneDirection::Both) {
            1
        } else {
            0
        };
        if parts.len() == forward_side.len() - offset {
            for (idx, part) in parts.into_iter().enumerate() {
                if part == "designated" {
                    forward_side[idx + offset].set_bus(locale)?;
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
                    backward_side[idx].set_bus(locale)?;
                }
            }
        }
    }

    Ok(())
}