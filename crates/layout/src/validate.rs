//! Structural checks of a parsed layout. Names of metrics, units, icons and fonts are
//! checked by `actionlay_render::diagnose`.
//!
//! Errors reject the layout; the renderer relies on validated layouts (finite, positive
//! sizes; opacities in 0..=1; parsable formats). Warnings flag parts this version keeps
//! but does not use (spec §6.4).
use std::collections::HashSet;
use std::fmt;

use crate::model::{Common, Extra, Node, Widget};
use crate::style::{OutlineOpt, ShadowOpt, TextStyleOpt, Theme};
use crate::{CURRENT_VERSION, Layout, format};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The layout loads; something is ignored or approximated.
    Warning,
    /// The layout is rejected.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// Where: `layout`, `theme`, `nodes[2].children[0] (speed)`.
    pub path: String,
    pub message: String,
}

impl Issue {
    pub fn error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn warning(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{s}: {}: {}", self.path, self.message)
    }
}

/// Every issue of `layout`, in document order.
pub fn validate(layout: &Layout) -> Vec<Issue> {
    let mut v = Validator {
        issues: Vec::new(),
        ids: HashSet::new(),
    };
    if layout.version == 0 {
        v.issues
            .push(Issue::error("layout", "version must be 1 or higher"));
    } else if layout.version > CURRENT_VERSION {
        v.issues.push(Issue::warning(
            "layout",
            format!(
                "written by a newer ActionLay (format version {}, this build reads {CURRENT_VERSION}): unknown parts are kept but not drawn",
                layout.version
            ),
        ));
    }
    let mut extra = layout.extra.clone();
    extra.remove("assets");
    v.extra("layout", &extra);
    if let Err(error) = crate::package::asset_names(layout) {
        v.issues
            .push(Issue::error("layout.assets", error.to_string()));
    }
    if let Some(theme) = &layout.theme {
        v.theme(theme);
    }
    for (i, node) in layout.nodes.iter().enumerate() {
        v.node(node, format!("nodes[{i}]"));
    }
    v.issues
}

struct Validator {
    issues: Vec<Issue>,
    ids: HashSet<String>,
}

impl Validator {
    fn bar(&mut self, path: &str, b: &crate::model::BarNode) {
        if b.metric.is_empty() {
            self.error(path, "metric must not be empty");
        }
        let [w, h] = b.size();
        self.positive(path, "size", w);
        self.positive(path, "size", h);
        let (min, max) = b.range();
        if !(min.is_finite() && max.is_finite() && max > min && (max - min).is_finite()) {
            self.error(path, "range must be finite with max greater than min");
        }
        if b.baseline.is_some_and(|v| !v.is_finite()) {
            self.error(path, "baseline must be finite");
        }
        if let Some(radius) = b.radius {
            self.non_negative(path, "radius", radius);
        }
        if let Some(secs) = b.stale_secs {
            self.non_negative(path, "stale_secs", secs);
        }
        if let Some(f) = &b.format
            && let Err(e) = format::parse(f)
        {
            self.error(path, e.to_string());
        }
        if let Some(style) = &b.value_style {
            self.style(&format!("{path}.value_style"), style);
        }
        if let Some(border) = &b.border {
            if let Some(w) = border.width {
                self.non_negative(path, "border width", w);
            }
            self.extra(&format!("{path}.border"), &border.extra);
        }
    }

    fn dial(&mut self, path: &str, d: &crate::model::DialNode) {
        if d.metric.is_empty() {
            self.error(path, "metric must not be empty");
        }
        self.positive(path, "diameter", d.diameter());
        self.positive(path, "thickness", d.thickness());
        if d.thickness() > d.diameter() * 0.2 {
            self.error(path, "thickness must not exceed 20% of diameter");
        }
        if let Some(secs) = d.stale_secs {
            self.non_negative(path, "stale_secs", secs);
        }
        if let Some(f) = &d.format
            && let Err(e) = format::parse(f)
        {
            self.error(path, e.to_string());
        }
        if let Some(style) = &d.value_style {
            self.style(&format!("{path}.value_style"), style);
        }
        if let Some(style) = &d.label_style {
            self.style(&format!("{path}.label_style"), style);
        }
    }

    fn error(&mut self, path: &str, message: impl Into<String>) {
        self.issues.push(Issue::error(path, message));
    }

    fn extra(&mut self, path: &str, extra: &Extra) {
        for key in extra.keys() {
            self.issues.push(Issue::warning(
                path,
                format!("unknown field `{key}`: kept, not used"),
            ));
        }
    }

    fn positive(&mut self, path: &str, what: &str, value: f32) {
        if !(value.is_finite() && value > 0.0) {
            self.error(
                path,
                format!("{what} must be a positive number (got {value})"),
            );
        }
    }

    fn non_negative(&mut self, path: &str, what: &str, value: f32) {
        if !(value.is_finite() && value >= 0.0) {
            self.error(
                path,
                format!("{what} must be zero or positive (got {value})"),
            );
        }
    }

    fn unit_interval(&mut self, path: &str, what: &str, value: f32) {
        // NaN is not contained in any range
        if !(0.0..=1.0).contains(&value) {
            self.error(
                path,
                format!("{what} must be between 0 and 1 (got {value})"),
            );
        }
    }

    fn finite_pair(&mut self, path: &str, what: &str, [x, y]: [f32; 2]) {
        if !(x.is_finite() && y.is_finite()) {
            self.error(path, format!("{what} must be finite"));
        }
    }

    fn theme(&mut self, theme: &Theme) {
        self.extra("theme", &theme.extra);
        if let Some(d) = theme.dim_opacity {
            self.unit_interval("theme", "dim_opacity", d);
        }
        if let Some(p) = &theme.palette {
            self.extra("theme.palette", &p.extra);
        }
        self.outline("theme", theme.outline.as_ref());
        self.shadow("theme", theme.shadow.as_ref());
    }

    fn outline(&mut self, path: &str, outline: Option<&OutlineOpt>) {
        let Some(o) = outline else { return };
        if let Some(w) = o.width {
            self.non_negative(path, "outline width", w);
        }
        self.extra(&format!("{path}.outline"), &o.extra);
    }

    fn shadow(&mut self, path: &str, shadow: Option<&ShadowOpt>) {
        let Some(s) = shadow else { return };
        if let Some(offset) = s.offset {
            self.finite_pair(path, "shadow offset", offset);
        }
        self.extra(&format!("{path}.shadow"), &s.extra);
    }

    fn style(&mut self, path: &str, style: &TextStyleOpt) {
        if let Some(size) = style.size {
            self.positive(path, "size", size);
        }
        self.outline(path, style.outline.as_ref());
        self.shadow(path, style.shadow.as_ref());
    }

    fn id(&mut self, path: &str, id: &str) {
        if id.is_empty() {
            self.error(path, "id must not be empty");
        } else if !self.ids.insert(id.to_string()) {
            self.error(path, format!("duplicate id `{id}`"));
        }
    }

    fn common(&mut self, path: &str, c: &Common) {
        if let Some(o) = c.opacity {
            self.unit_interval(path, "opacity", o);
        }
        if let Some(offset) = c.offset {
            self.finite_pair(path, "offset", offset);
        }
        if let Some(offset) = c.offset_relative {
            self.finite_pair(path, "offset_relative", offset);
        }
    }

    fn node(&mut self, node: &Node, path: String) {
        let path = match node.id() {
            Some(id) if !id.is_empty() => format!("{path} ({id})"),
            _ => path,
        };
        if let Some(id) = node.id() {
            self.id(&path, id);
        }
        let w = match node {
            Node::Unknown(_) => {
                self.issues.push(Issue::warning(
                    &path,
                    format!("unknown node type `{}`: kept, not drawn", node.type_name()),
                ));
                return;
            }
            Node::Known(w) => w,
        };
        self.common(&path, w.common());
        self.extra(&path, w.extra());
        match w {
            Widget::Chart(c) | Widget::GradientChart(c) => {
                if c.stale_secs.is_some_and(|v| !v.is_finite() || v < 0.0) {
                    self.error(&path, "stale_secs must be non-negative and finite");
                }
                if c.metric.is_empty() {
                    self.error(&path, "metric must not be empty");
                }
                for v in c.size() {
                    self.positive(&path, "size", v);
                }
                if c.seconds
                    .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 86400.0)
                {
                    self.error(&path, "seconds must be in 0..=86400");
                }
                if c.samples.is_some_and(|v| !(2..=2048).contains(&v)) {
                    self.error(&path, "samples must be in 2..=2048");
                }
                if c.min.is_some_and(|v| !v.is_finite())
                    || c.max.is_some_and(|v| !v.is_finite())
                    || c.min
                        .zip(c.max)
                        .is_some_and(|(a, b)| b <= a || !(b - a).is_finite())
                {
                    self.error(&path, "invalid chart range");
                }
                if let Some(v) = c.radius {
                    self.non_negative(&path, "radius", v);
                }
                if let Some(v) = c.stroke_width {
                    self.positive(&path, "stroke_width", v);
                }
                if let Some(v) = &c.value_style {
                    self.style(&path, v);
                }
            }
            Widget::Map(m) => {
                if let Some(style) = &m.label_style {
                    self.style(&path, style);
                }
                if m.stale_secs.is_some_and(|v| !v.is_finite() || v < 0.0) {
                    self.error(&path, "stale_secs must be non-negative and finite");
                }
                for v in m.size() {
                    self.positive(&path, "size", v);
                }
                if m.route_coverage
                    .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 1.0)
                {
                    self.error(
                        &path,
                        "route_coverage must be greater than zero and at most one",
                    );
                }
                if m.zoom.is_some_and(|v| v > 19) {
                    self.error(&path, "zoom must not exceed 19");
                }
                if let Some(v) = m.radius {
                    self.non_negative(&path, "radius", v);
                }
                if let Some(v) = m.route_width {
                    self.positive(&path, "route_width", v);
                }
                if let Some(v) = m.marker_radius {
                    self.positive(&path, "marker_radius", v);
                }
                if let Some(v) = m.opacity_tiles {
                    self.unit_interval(&path, "opacity_tiles", v);
                }
            }
            Widget::GMeter(g) => {
                if g.stale_secs.is_some_and(|v| !v.is_finite() || v < 0.0) {
                    self.error(&path, "stale_secs must be non-negative and finite");
                }
                self.positive(&path, "diameter", g.diameter());
                if g.range.is_some_and(|v| !v.is_finite() || v <= 0.0) {
                    self.error(&path, "range must be positive and finite");
                }
                if g.rings.is_some_and(|v| !(1..=12).contains(&v)) {
                    self.error(&path, "rings must be in 1..=12");
                }
                if g.trail_secs
                    .is_some_and(|v| !v.is_finite() || !(0.0..=30.0).contains(&v))
                {
                    self.error(&path, "trail_secs must be in 0..=30");
                }
                if g.rotation.is_some_and(|v| !v.is_finite()) {
                    self.error(&path, "rotation must be finite");
                }
                if let Some(v) = &g.value_style {
                    self.style(&path, v);
                }
            }
            Widget::Gauge(g) => {
                self.dial(&path, &g.dial);
                let (min, max) = g.range();
                if !(min.is_finite() && max.is_finite() && max > min && (max - min).is_finite()) {
                    self.error(&path, "range must be finite with max greater than min");
                }
                let (start, sweep) = g.angles();
                if !start.is_finite() || !(-360.0..=360.0).contains(&start) {
                    self.error(&path, "start_angle must be between -360 and 360");
                }
                if !(sweep.is_finite() && sweep > 0.0 && sweep <= 360.0) {
                    self.error(&path, "sweep_angle must be greater than 0 and at most 360");
                }
                if g.ticks.is_some_and(|n| n > 72) {
                    self.error(&path, "ticks must not exceed 72");
                }
                if let Some(zones) = &g.zones {
                    let mut previous = min;
                    if zones.is_empty() {
                        self.error(&path, "zones must not be empty");
                    }
                    for zone in zones {
                        if !zone.up_to.is_finite() || zone.up_to <= previous || zone.up_to > max {
                            self.error(&path, "gauge zones must increase strictly within range");
                        }
                        previous = zone.up_to;
                    }
                    if !zones.is_empty() && previous != max {
                        self.error(&path, "last gauge zone must end at max");
                    }
                }
            }
            Widget::Compass(c) => {
                self.dial(&path, &c.dial);
                if let Some(f) = &c.smoothing {
                    for (key, v, limit) in [
                        ("seconds", f.seconds, 30.0),
                        ("deadband", f.deadband, 180.0),
                        ("max_rate", f.max_rate, 3600.0),
                        ("min_speed", f.min_speed, 100.0),
                    ] {
                        if v.is_some_and(|v| !v.is_finite() || v < 0.0 || v > limit) {
                            self.error(&path, format!("invalid smoothing {key}"));
                        }
                    }
                }
            }
            Widget::Bar(b) => self.bar(&path, b),
            Widget::ZoneBar(z) => {
                self.bar(&path, &z.bar);
                if let Some(zones) = &z.zones {
                    let (mut previous, max) = z.bar.range();
                    if zones.is_empty() {
                        self.error(&path, "zones must not be empty");
                    }
                    for (i, zone) in zones.iter().enumerate() {
                        let path = format!("{path}.zones[{i}]");
                        if !(zone.up_to.is_finite() && zone.up_to > previous && zone.up_to <= max) {
                            self.error(&path, "up_to must increase strictly within the bar range");
                        }
                        previous = zone.up_to;
                        self.extra(&path, &zone.extra);
                    }
                    if !zones.is_empty() && previous != max {
                        self.error(&path, "last zone must end at max");
                    }
                }
            }
            Widget::Group(g) => {
                if let Some([sw, sh]) = g.size {
                    self.positive(&path, "size", sw);
                    self.positive(&path, "size", sh);
                }
            }
            Widget::Frame(f) => {
                self.positive(&path, "size", f.size[0]);
                self.positive(&path, "size", f.size[1]);
                if let Some(r) = f.radius {
                    self.non_negative(&path, "radius", r);
                }
                if let Some(b) = &f.border {
                    if let Some(bw) = b.width {
                        self.non_negative(&path, "border width", bw);
                    }
                    self.extra(&format!("{path}.border"), &b.extra);
                }
            }
            Widget::Text(t) => self.style(&path, &t.style),
            Widget::Metric(m) => {
                if m.metric.is_empty() {
                    self.error(&path, "metric must not be empty");
                }
                if let Some(f) = &m.format
                    && let Err(e) = format::parse(f)
                {
                    self.error(&path, e.to_string());
                }
                if let Some(s) = m.stale_secs {
                    self.non_negative(&path, "stale_secs", s);
                }
                self.style(&path, &m.style);
            }
            Widget::MetricUnit(m) => {
                if let Some(secs) = m.stale_secs {
                    self.non_negative(&path, "stale_secs", secs);
                }
                if m.metric.is_empty() {
                    self.error(&path, "metric must not be empty");
                }
                self.style(&path, &m.style);
            }
            Widget::Datetime(d) => {
                if let Some(f) = &d.format
                    && !format::is_valid_strftime(f)
                {
                    self.error(&path, format!("invalid strftime format `{f}`"));
                }
                self.style(&path, &d.style);
            }
            Widget::Icon(i) => {
                if i.icon.is_empty() {
                    self.error(&path, "icon must not be empty");
                }
                if let Some(s) = i.size {
                    self.positive(&path, "size", s);
                }
            }
            Widget::GpsLockIcon(g) => {
                if let Some(secs) = g.stale_secs {
                    self.non_negative(&path, "stale_secs", secs);
                }
                if let Some(s) = g.size {
                    self.positive(&path, "size", s);
                }
            }
        }
        for (i, child) in w.children().iter().enumerate() {
            self.node(child, format!("{path}.children[{i}]"));
        }
    }
}
