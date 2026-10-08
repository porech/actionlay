//! Source metadata needed by time-dependent widgets. Shared by every runtime.
use crate::model::MapRoute;
use crate::{Layout, Node, Widget};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Requirement {
    pub start: f64,
    pub end: f64,
    /// Full-source validation is required, even when the last packet ends early.
    pub full: bool,
}

impl Widget {
    pub fn history_requirement(&self, time: f64, duration: f64) -> Option<Requirement> {
        let time = time.clamp(0.0, duration.max(0.0));
        let prefix = || Requirement {
            start: 0.0,
            end: time,
            full: false,
        };
        let window = |seconds: f64| Requirement {
            // Context for derived metrics and interpolation at the left edge.
            start: (time - seconds - 1.0).max(0.0),
            end: time,
            full: false,
        };
        let cumulative =
            |metric: &str| matches!(metric, "odo" | "codo" | "accel.lon" | "accel.lat");
        Some(match self {
            Widget::Map(map) if map.needs_full_track() => Requirement {
                start: 0.0,
                end: duration,
                full: true,
            },
            Widget::Map(map) if map.route_mode == Some(MapRoute::Past) => prefix(),
            Widget::Chart(chart) | Widget::GradientChart(chart) if chart.journey == Some(true) => {
                Requirement {
                    start: 0.0,
                    end: duration,
                    full: true,
                }
            }
            Widget::Chart(chart) | Widget::GradientChart(chart) => {
                let seconds = chart.seconds.unwrap_or(60.0).max(0.0);
                let mut range = if cumulative(&chart.metric) {
                    prefix()
                } else {
                    window(seconds)
                };
                // Match the chart's initial fixed-width time axis.
                range.end = time.max(seconds).min(duration);
                range
            }
            // Peaks and vehicle IMU calibration depend on the played prefix.
            Widget::GMeter(_) => prefix(),
            // The causal heading filter carries state through the whole prefix.
            Widget::Compass(compass)
                if compass
                    .smoothing
                    .as_ref()
                    .is_some_and(|f| f.enabled != Some(false)) =>
            {
                prefix()
            }
            Widget::Compass(compass) if cumulative(&compass.dial.metric) => prefix(),
            Widget::Metric(metric) if cumulative(&metric.metric) => prefix(),
            Widget::Bar(bar) if cumulative(&bar.metric) => prefix(),
            Widget::ZoneBar(bar) if cumulative(&bar.bar.metric) => prefix(),
            Widget::Gauge(gauge) if cumulative(&gauge.dial.metric) => prefix(),
            _ => return None,
        })
    }
}

impl Layout {
    pub fn history_requirements(&self, time: f64, duration: f64) -> Vec<Requirement> {
        fn collect(nodes: &[Node], time: f64, duration: f64, out: &mut Vec<Requirement>) {
            for node in nodes {
                let Node::Known(widget) = node else { continue };
                if widget.common().visible == Some(false) || widget.common().opacity == Some(0.0) {
                    continue;
                }
                if let Some(range) = widget.history_requirement(time, duration) {
                    out.push(range);
                }
                collect(widget.children(), time, duration, out);
            }
        }
        let mut ranges = Vec::new();
        collect(&self.nodes, time, duration, &mut ranges);
        if ranges.iter().any(|r| r.full) {
            return vec![Requirement {
                start: 0.0,
                end: duration,
                full: true,
            }];
        }
        ranges.retain(|r| r.end > r.start);
        ranges.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut merged: Vec<Requirement> = Vec::new();
        for range in ranges {
            if let Some(last) = merged.last_mut().filter(|last| range.start <= last.end) {
                last.end = last.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn requirements(nodes: &str, time: f64) -> Vec<Requirement> {
        Layout::from_json(&format!(r#"{{"version":1,"nodes":{nodes}}}"#))
            .unwrap()
            .layout
            .history_requirements(time, 300.0)
    }
    #[test]
    fn chart_seek_requests_its_window_and_journey_requests_validated_full_source() {
        assert_eq!(
            requirements(
                r#"[{"type":"gradient_chart","metric":"alt","seconds":60}]"#,
                180.0
            ),
            vec![Requirement {
                start: 119.0,
                end: 180.0,
                full: false
            }]
        );
        assert_eq!(
            requirements(r#"[{"type":"chart","metric":"alt","seconds":60}]"#, 0.0),
            vec![Requirement {
                start: 0.0,
                end: 60.0,
                full: false
            }]
        );
        assert_eq!(
            requirements(
                r#"[{"type":"gradient_chart","metric":"alt","journey":true}]"#,
                180.0
            ),
            vec![Requirement {
                start: 0.0,
                end: 300.0,
                full: true
            }]
        );
    }
    #[test]
    fn hidden_ancestors_suppress_requirements_and_overlapping_history_is_merged() {
        assert!(requirements(r#"[{"type":"frame","size":[400,300],"opacity":0,"children":[{"type":"chart","metric":"alt","journey":true}]}]"#, 180.0).is_empty());
        assert_eq!(requirements(r#"[{"type":"chart","metric":"alt","seconds":60},{"type":"chart","metric":"speed","seconds":20}]"#,180.0).len(),1);
        assert_eq!(requirements(r#"[{"type":"frame","size":[400,300],"children":[{"type":"map","route_mode":"full"}]}]"#,180.0)[0].end,300.0);
    }
    #[test]
    fn all_stateful_widgets_request_history_and_instantaneous_widgets_do_not() {
        for node in [
            r#"{"type":"g_meter","trail_secs":8,"show_peaks":true}"#,
            r#"{"type":"compass","metric":"heading","smoothing":{}}"#,
            r#"{"type":"metric","metric":"odo"}"#,
            r#"{"type":"gauge","metric":"codo"}"#,
            r#"{"type":"bar","metric":"accel.lon"}"#,
            r#"{"type":"zone_bar","metric":"accel.lat"}"#,
            r#"{"type":"map","route_mode":"past"}"#,
        ] {
            assert_eq!(
                requirements(&format!("[{node}]"), 180.0),
                vec![Requirement {
                    start: 0.0,
                    end: 180.0,
                    full: false
                }],
                "{node}"
            );
        }
        assert!(requirements(r#"[{"type":"metric","metric":"speed"},{"type":"compass","metric":"heading","smoothing":{"enabled":false}},{"type":"map","route_mode":"none","zoom_mode":"fixed"}]"#,180.0).is_empty());
    }
}
