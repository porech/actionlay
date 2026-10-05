//! GPL upstream XML → native responsive layout. Losses are reported, never silent.
use crate::{Layout, validate::Issue};
use roxmltree::Node;
use serde_json::{Value, json};

pub struct Imported {
    pub layout: Layout,
    pub warnings: Vec<Issue>,
}
pub fn reference_size(name: &str) -> Option<[u32; 2]> {
    let parts: Vec<_> = name
        .split(|c: char| !c.is_ascii_digit() && c != 'x')
        .collect();
    for part in parts {
        if let Some((w, h)) = part.split_once('x')
            && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
        {
            return Some([w, h]);
        }
    }
    if name.contains("2160") {
        Some([3840, 2160])
    } else if name.contains("1080") {
        Some([1920, 1080])
    } else {
        None
    }
}

pub fn xml(text: &str, name: &str, resolution: [u32; 2]) -> Result<Imported, String> {
    if text.len() > 2 * 1024 * 1024 {
        return Err("XML layout exceeds 2 MiB".into());
    }
    let [w, h] = resolution;
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return Err("Reference resolution must be in 1..=16384".into());
    }
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    if doc.root_element().tag_name().name() != "layout" {
        return Err("Expected a <layout> root".into());
    }
    let mut converter = Converter {
        scale: 1080.0 / h as f64,
        width: w as f64 * 1080.0 / h as f64,
        warnings: Vec::new(),
    };
    let mut nodes = converter.children(doc.root_element(), [0.0, 0.0])?;
    for node in &mut nodes {
        converter.anchor(node);
    }
    let value = json!({"version":1,"name":name.trim_end_matches(".xml"),"design_aspect":format!("{w}:{h}"),"nodes":nodes});
    let loaded = Layout::from_json(&value.to_string()).map_err(|e| e.to_string())?;
    converter.warnings.extend(loaded.warnings);
    Ok(Imported {
        layout: loaded.layout,
        warnings: converter.warnings,
    })
}
struct Converter {
    scale: f64,
    width: f64,
    warnings: Vec<Issue>,
}
impl Converter {
    fn note(&mut self, e: Node, message: impl Into<String>) {
        self.warnings.push(Issue::warning(
            e.attribute("name")
                .unwrap_or(e.attribute("type").unwrap_or(e.tag_name().name())),
            message,
        ));
    }
    fn num(&mut self, e: Node, key: &str, default: f64) -> f64 {
        let Some(s) = e.attribute(key) else {
            return default;
        };
        match s.parse::<f64>() {
            Ok(v) if v.is_finite() => v,
            _ => {
                self.note(e, format!("Invalid {key}={s}, using {default}"));
                default
            }
        }
    }
    fn color(&mut self, e: Node, key: &str) -> Option<String> {
        let text = e.attribute(key)?;
        if text.starts_with('#') {
            return Some(text.into());
        }
        let values: Option<Vec<u8>> = text.split(',').map(|s| s.trim().parse().ok()).collect();
        match values {
            Some(v) if v.len() == 3 || v.len() == 4 => Some(format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                v[0],
                v[1],
                v[2],
                *v.get(3).unwrap_or(&255)
            )),
            _ => {
                self.note(e, format!("Invalid colour {key}, using theme"));
                None
            }
        }
    }
    fn units(&mut self, e: Node) -> Option<String> {
        let s = e.attribute("units")?;
        let u = match s {
            "kph" => "kmh",
            "miles" => "mi",
            "nautical_miles" => "nmi",
            "feet" => "ft",
            "gravity" => "none",
            "G" => "g",
            "rpm" | "spm" | "W" => "none",
            "speed" | "alt" | "temp" => return None,
            "kW" => {
                self.note(
                    e,
                    "kW is converted to the native power unit; check the value format",
                );
                return None;
            }
            _ => s,
        };
        Some(u.into())
    }
    fn children(&mut self, e: Node, origin: [f64; 2]) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for child in e.children().filter(Node::is_element) {
            let offset = [
                origin[0] + self.num(child, "x", 0.0) * self.scale,
                origin[1] + self.num(child, "y", 0.0) * self.scale,
            ];
            match child.tag_name().name() {
                "translate" | "composite" => out.extend(self.children(child, offset)?),
                "frame" => {
                    let children = self.children(child, [0.0, 0.0])?;
                    let size = [
                        self.num(child, "width", 256.0) * self.scale,
                        self.num(child, "height", 256.0) * self.scale,
                    ];
                    let mut n = json!({"type":"frame","offset":offset,"size":size,"children":children,"radius":self.num(child,"cr",12.0)*self.scale});
                    if let Some(color) = self.color(child, "bg") {
                        n["fill"] = json!(color);
                    }
                    if let Some(opacity) = child.attribute("opacity") {
                        n["opacity"] = json!(
                            opacity
                                .parse::<f64>()
                                .map_err(|_| "Invalid frame opacity")?
                        );
                    }
                    out.push(n);
                }
                "component" => out.push(self.component(child, offset)?),
                tag => return Err(format!("Unknown XML tag <{tag}>")),
            }
        }
        Ok(out)
    }
    fn component(&mut self, e: Node, offset: [f64; 2]) -> Result<Value, String> {
        let kind = e.attribute("type").ok_or("Component is missing type")?;
        let metric =
            e.attribute("metric")
                .unwrap_or(if kind == "compass" || kind == "compass-arrow" {
                    "heading"
                } else {
                    "speed"
                });
        let diameter = self.num(e, "size", 256.0) * self.scale;
        let mut n = match kind {
            "text" => json!({"type":"text","text":e.text().unwrap_or("").trim()}),
            "metric" if metric == "timestamp" => {
                self.note(e, "timestamp uses native datetime formatting");
                json!({"type":"datetime"})
            }
            "metric" | "metric_unit" => json!({"type":kind,"metric":metric}),
            "datetime" => json!({"type":"datetime"}),
            "gps-lock-icon" => json!({"type":"gps_lock_icon","size":diameter}),
            "icon" => {
                let file = e.attribute("file").unwrap_or("");
                let icon = match file {
                    "mountain.png" => "altitude",
                    "slope-triangle.png" => "gradient",
                    "gauge.png" => "speed",
                    "heartbeat.png" => "heart",
                    "power.png" => "power",
                    "thermometer.png" => "temperature",
                    _ => {
                        self.note(e, format!("Unknown icon {file}: replaced by the GPS icon"));
                        "gps"
                    }
                };
                json!({"type":"icon","icon":icon,"size":diameter})
            }
            "bar" | "zone-bar" => {
                let min = self.num(e, "min", 0.0);
                let max = self.num(e, "max", if kind == "zone-bar" { 400.0 } else { 100.0 });
                let mut n = json!({"type":if kind=="bar"{"bar"}else{"zone_bar"},"metric":metric,"size":[self.num(e,"width",400.0)*self.scale,self.num(e,"height",30.0)*self.scale],"min":min,"max":max,"show_value":false});
                if let Some(v) = self.color(e, "bar") {
                    n["fill"] = json!(v);
                }
                if kind == "zone-bar" {
                    let mut zones = Vec::new();
                    let mut last = min;
                    for (key, color, fallback) in [
                        ("z1", "z0-rgb", "#ffffff"),
                        ("z2", "z1-rgb", "#43eb34"),
                        ("z3", "z2-rgb", "#f0e813"),
                    ] {
                        let bound = self.num(
                            e,
                            key,
                            match key {
                                "z1" => 120.0,
                                "z2" => 160.0,
                                _ => 200.0,
                            },
                        );
                        if bound > last && bound < max {
                            zones.push(json!({"up_to":bound,"color":self.color(e,color).unwrap_or(fallback.into())}));
                            last = bound;
                        }
                    }
                    zones.push(json!({"up_to":max,"color":self.color(e,"z3-rgb").unwrap_or("#cf1302".into())}));
                    n["zones"] = json!(zones);
                }
                n
            }
            "chart" | "gradient_chart" => {
                let metric = if e.attribute("metric").is_none()
                    && (e.attribute("name") == Some("gradient_chart")
                        || e.attribute("units") == Some("alt"))
                {
                    "alt"
                } else {
                    metric
                };
                json!({"type":if kind=="gradient_chart" || e.attribute("name")==Some("gradient_chart"){"gradient_chart"}else{"chart"},"metric":metric,"size":[self.num(e,"width",256.0)*self.scale,self.num(e,"height",64.0)*self.scale],"samples":self.num(e,"samples",256.0).clamp(2.0,2048.0) as u32,"seconds":self.num(e,"seconds",60.0),"show_value":e.attribute("values")!=Some("false")})
            }
            "moving_map" | "journey_map" | "moving_journey_map" | "cairo_circuit_map"
            | "cairo-circuit-map" | "circuit_map" => {
                json!({"type":"map","mode":match kind{"moving_map"=>"moving","journey_map"=>"journey","moving_journey_map"=>"moving_journey",_=>"circuit"},"size":[diameter,diameter],"zoom":self.num(e,"zoom",15.0) as u8,"radius":self.num(e,"corner_radius",12.0)*self.scale})
            }
            "compass" | "compass-arrow" => {
                json!({"type":"compass","metric":"heading","diameter":diameter,"mode":if kind=="compass"{"rose"}else{"arrow"},"rotate_rose":kind=="compass","smoothing":{"seconds":0.5,"deadband":1.5,"max_rate":120.0,"min_speed":1.5}})
            }
            "asi"
            | "msi"
            | "msi2"
            | "cairo-gauge-marker"
            | "cairo-gauge-round-annotated"
            | "cairo-gauge-arc-annotated"
            | "cairo-gauge-donut" => {
                self.note(e, format!("{kind} converted to native gauge styling"));
                let length = self.num(e, "length", 270.0);
                let default_max = self.num(e, "vne", 180.0);
                let mut n = json!({"type":"gauge","metric":metric,"diameter":diameter,"mode":match kind{"cairo-gauge-marker"=>"marker","cairo-gauge-donut"=>"donut","cairo-gauge-arc-annotated"|"msi2"=>"arc","msi" if e.attribute("needle")==Some("0")=>"arc",_=>"needle"},"start_angle":self.num(e,"start",135.0),"sweep_angle":length.abs().clamp(1.0,360.0),"clockwise":length>=0.0,"max":self.num(e,"end",default_max),"value_style":{"size":self.num(e,"textsize",26.0)*self.scale}});
                if kind == "asi" {
                    let max = n["max"].as_f64().unwrap();
                    let mut zones = Vec::new();
                    for (key, default, color) in [
                        ("vs0", 40.0, "#ffffff"),
                        ("vfe", 103.0, "#64dca5"),
                        ("vno", 126.0, "#ffffff"),
                    ] {
                        let v = self.num(e, key, default);
                        if v < max {
                            zones.push(json!({"up_to":v,"color":color}));
                        }
                    }
                    zones.push(json!({"up_to":max,"color":"#ffb300"}));
                    n["zones"] = json!(zones);
                }
                if let Some(sectors) = e.attribute("sectors") {
                    n["ticks"] = json!(sectors.parse::<u32>().map_err(|_| "Invalid sectors")?);
                    n["show_labels"] = json!(false);
                }
                n
            }
            other => {
                self.note(
                    e,
                    format!("Unsupported component {other}: preserved, not drawn"),
                );
                json!({"type":"upstream_unknown","source_type":other,"source_xml":e.document().input_text()[e.range()],"source_attributes":e.attributes().map(|a|(a.name().to_string(),json!(a.value()))).collect::<serde_json::Map<String,Value>>()})
            }
        };
        n["offset"] = json!(offset);
        if matches!(
            n["type"].as_str(),
            Some("text" | "metric" | "metric_unit" | "datetime")
        ) {
            n["size"] = json!(self.num(e, "size", 16.0) * self.scale);
            if let Some(color) = self.color(e, "rgb") {
                n["color"] = json!(color);
            }
            if e.attribute("align") == Some("centre") {
                n["anchor"] = json!("top");
            } else if e.attribute("align") == Some("right") {
                n["anchor"] = json!("top-right");
            }
            if let Some(f) = e.attribute("format") {
                if n["type"] == "datetime" {
                    n["format"] = json!(f);
                } else if f.starts_with('.') && f.ends_with('f') {
                    n["format"] = json!(format!("{{value:{}}}", f.trim_end_matches('f')));
                } else if f == "pace" {
                    n["format"] = json!("{value:pace}");
                } else {
                    self.note(
                        e,
                        format!("Format {f} replaced by the native numeric format"),
                    );
                }
            } else if let Some(dp) = e.attribute("dp") {
                n["format"] = json!(format!("{{value:.{dp}}}"));
            }
        }
        if let Some(units) = self.units(e)
            && !matches!(
                n["type"].as_str(),
                Some("text" | "datetime" | "icon" | "map")
            )
        {
            n["units"] = json!(units);
        }
        for (source, target) in [("fill", "fill"), ("bg", "background")] {
            if matches!(n["type"].as_str(), Some("chart" | "gradient_chart"))
                && let Some(color) = self.color(e, source)
            {
                n[target] = json!(color);
            }
        }
        // Every unsupported attribute is reported, including custom font and
        // styling controls that are not in the shipped XML fixtures.
        for a in e.attributes() {
            if !matches!(
                a.name(),
                "type"
                    | "name"
                    | "x"
                    | "y"
                    | "size"
                    | "metric"
                    | "units"
                    | "format"
                    | "dp"
                    | "align"
                    | "file"
                    | "min"
                    | "max"
                    | "width"
                    | "height"
                    | "bar"
                    | "z1"
                    | "z2"
                    | "z3"
                    | "z0-rgb"
                    | "z1-rgb"
                    | "z2-rgb"
                    | "z3-rgb"
                    | "values"
                    | "samples"
                    | "seconds"
                    | "fill"
                    | "bg"
                    | "zoom"
                    | "corner_radius"
                    | "length"
                    | "start"
                    | "vne"
                    | "end"
                    | "textsize"
                    | "vs0"
                    | "vfe"
                    | "vno"
                    | "sectors"
                    | "needle"
                    | "rgb"
            ) {
                self.note(
                    e,
                    format!(
                        "{}={} uses native theme/rendering defaults",
                        a.name(),
                        a.value()
                    ),
                );
            }
        }
        if matches!(
            n["type"].as_str(),
            Some("text" | "metric" | "metric_unit" | "datetime")
        ) {
            // XML text alignment refers to its coordinate, not to the whole
            // parent frame. Keep a zero-sized origin so native anchors align
            // the actual shaped text without guessing its width.
            n["offset"] = json!([0.0, 0.0]);
            return Ok(json!({"type":"group", "offset":offset, "children":[n]}));
        }
        Ok(n)
    }
    fn anchor(&mut self, node: &mut Value) {
        let offset = node["offset"]
            .as_array()
            .map(|v| [v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0)])
            .unwrap_or([0.0, 0.0]);
        let size = if node["type"] == "group" {
            [0.0, 0.0]
        } else if let Some(v) = node["size"].as_array() {
            [v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0)]
        } else if let Some(v) = node["diameter"].as_f64() {
            [v, v]
        } else {
            let font = node["size"].as_f64().unwrap_or(20.0);
            [font * 3.0, font]
        };
        let right = offset[0] + size[0] / 2.0 > self.width / 2.0;
        let bottom = offset[1] + size[1] / 2.0 > 540.0;
        node["anchor"] = json!(match (right, bottom) {
            (true, true) => "bottom-right",
            (true, false) => "top-right",
            (false, true) => "bottom-left",
            _ => "top-left",
        });
        node.as_object_mut().unwrap().remove("offset");
        node["offset_relative"] = json!([
            (offset[0] + if right { size[0] - self.width } else { 0.0 }) / self.width,
            (offset[1] + if bottom { size[1] - 1080.0 } else { 0.0 }) / 1080.0
        ]);
    }
}
