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
    v.extra("layout", &layout.extra);
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
