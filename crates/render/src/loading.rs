//! Loading status and placement only. Badges are drawn by the UI, never exports.
use crate::HitBox;
use actionlay_layout::geom::Rect;
use actionlay_layout::{Layout, Node};
use actionlay_telemetry::Telemetry;

#[derive(Debug, Clone, PartialEq)]
pub struct LoadingRegion {
    pub rect: Rect,
    pub fraction: f64,
    pub missing: Vec<(f64, f64)>,
    pub requirement: actionlay_layout::history::Requirement,
}

pub fn regions(
    layout: &Layout,
    telemetry: &Telemetry,
    time: f64,
    duration: f64,
    boxes: &[HitBox],
) -> Vec<LoadingRegion> {
    boxes
        .iter()
        .filter_map(|hit| {
            let mut nodes = layout.nodes.as_slice();
            let mut selected = None;
            for &index in &hit.path {
                let Node::Known(widget) = nodes.get(index)? else {
                    return None;
                };
                if widget.common().visible == Some(false) || widget.common().opacity == Some(0.0) {
                    return None;
                }
                selected = Some(widget);
                nodes = widget.children();
            }
            let requirement = selected?.history_requirement(time, duration)?;
            let fraction = telemetry.read_fraction(requirement.start, requirement.end);
            let ready = if requirement.full {
                telemetry.is_complete()
            } else {
                fraction >= 1.0 - 1e-9
            };
            (!ready).then_some(LoadingRegion {
                rect: hit.rect,
                fraction,
                missing: telemetry
                    .unread_source_ranges(requirement.end)
                    .into_iter()
                    .filter_map(|(a, b)| {
                        let a = a.max(requirement.start);
                        (b > a).then_some((a, b))
                    })
                    .collect(),
                requirement,
            })
        })
        .collect()
}
