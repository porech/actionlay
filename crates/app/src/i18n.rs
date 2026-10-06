//! Embedded interface catalogues. File formats, metric IDs and user data stay stable.
use eframe::egui;
use regex::Regex;
use std::{
    collections::HashMap,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub struct Language {
    pub code: &'static str,
    pub native: &'static str,
    pub english: &'static str,
}
macro_rules! languages {
    ($($code:literal => ($native:literal, $english:literal)),+ $(,)?) => {
        pub const LANGUAGES: &[Language] = &[$(Language { code: $code, native: $native, english: $english }),+];
        const JSON: &[&str] = &[$(include_str!(concat!("../locales/", $code, ".json"))),+];
    };
}
languages! {
    "en" => ("English", "English"), "it" => ("Italiano", "Italian"),
    "fr" => ("Français", "French"), "de" => ("Deutsch", "German"),
    "es" => ("Español", "Spanish"), "ca" => ("Català", "Catalan"),
    "pt-BR" => ("Português (Brasil)", "Portuguese (Brazil)"),
    "pt-PT" => ("Português (Portugal)", "Portuguese (Portugal)"),
    "nl" => ("Nederlands", "Dutch"), "sv" => ("Svenska", "Swedish"),
    "da" => ("Dansk", "Danish"), "nb" => ("Norsk bokmål", "Norwegian"),
    "fi" => ("Suomi", "Finnish"), "pl" => ("Polski", "Polish"),
    "cs" => ("Čeština", "Czech"), "sk" => ("Slovenčina", "Slovak"),
    "hu" => ("Magyar", "Hungarian"), "ro" => ("Română", "Romanian"),
    "bg" => ("Български", "Bulgarian"), "hr" => ("Hrvatski", "Croatian"),
    "sl" => ("Slovenščina", "Slovenian"), "sr" => ("Српски", "Serbian"),
    "ru" => ("Русский", "Russian"), "uk" => ("Українська", "Ukrainian"),
    "el" => ("Ελληνικά", "Greek"), "tr" => ("Türkçe", "Turkish"),
    "ar" => ("العربية", "Arabic"), "he" => ("עברית", "Hebrew"),
    "id" => ("Bahasa Indonesia", "Indonesian"), "vi" => ("Tiếng Việt", "Vietnamese"),
    "ja" => ("日本語", "Japanese"), "ko" => ("한국어", "Korean"),
    "zh-Hans" => ("简体中文", "Chinese (Simplified)"),
    "zh-Hant" => ("繁體中文", "Chinese (Traditional)"),
}
static CURRENT: AtomicUsize = AtomicUsize::new(0);
struct Catalogue {
    logical: HashMap<String, String>,
    visual: HashMap<String, String>,
}
fn catalogues() -> &'static [Catalogue] {
    static ALL: OnceLock<Vec<Catalogue>> = OnceLock::new();
    ALL.get_or_init(|| {
        JSON.iter()
            .map(|data| {
                let logical: HashMap<String, String> =
                    serde_json::from_str(data).expect("validated language catalogue");
                let visual = logical
                    .iter()
                    .map(|(key, value)| (key.clone(), visual_text(value)))
                    .collect();
                Catalogue { logical, visual }
            })
            .collect()
    })
}
/// Native menus/dialogs use logical Unicode; the OS performs shaping and bidi.
pub fn native_text(source: &str) -> &str {
    catalogues()[CURRENT.load(Ordering::Relaxed)]
        .logical
        .get(source)
        .map_or(source, String::as_str)
}
/// egui's glyph renderer needs visual-order text for right-to-left scripts.
pub fn text(source: &str) -> &str {
    catalogues()[CURRENT.load(Ordering::Relaxed)]
        .visual
        .get(source)
        .map_or(source, String::as_str)
}
fn rtl(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x0590..=0x08ff | 0xfb1d..=0xfdff | 0xfe70..=0xfeff))
}
fn visual_text(text: &str) -> String {
    if !rtl(text) {
        return text.to_owned();
    }
    let shaped = arabic_reshaper::arabic_reshape(text);
    let bidi = unicode_bidi::BidiInfo::new(&shaped, None);
    bidi.paragraphs
        .iter()
        .map(|p| bidi.reorder_line(p, p.range.clone()).into_owned())
        .collect::<Vec<_>>()
        .join("")
}
/// Wrap logical RTL paragraphs before bidi reordering, using the available width.
pub fn ui_text(ui: &egui::Ui, source: impl AsRef<str>) -> String {
    let source = source.as_ref();
    let logical = translate_message(source);
    wrap_visual(ui, &logical)
}
/// User-defined layout names, file paths and editable data are never translated.
pub fn user_text(ui: &egui::Ui, source: impl AsRef<str>) -> String {
    wrap_visual(ui, source.as_ref())
}
fn wrap_visual(ui: &egui::Ui, logical: &str) -> String {
    if !rtl(logical) {
        return logical.to_owned();
    }
    let width = (ui.available_width() - 32.0).clamp(180.0, 640.0);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut lines = Vec::new();
    for paragraph in logical.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            let size = ui.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(visual_text(&candidate), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            if !line.is_empty() && size > width {
                lines.push(visual_text(&line));
                line = word.to_owned();
            } else {
                line = candidate;
            }
        }
        lines.push(visual_text(&line));
    }
    lines.join("\n")
}
struct Pattern {
    source: String,
    regex: Regex,
}
fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let fields = Regex::new(r"\{[^{}]*\}").unwrap();
        let keys: Vec<String> =
            serde_json::from_str(include_str!("../locales/formatted.json")).unwrap();
        keys.into_iter()
            .map(|source| {
                let mut regex = String::from("(?s)^");
                let mut end = 0;
                for field in fields.find_iter(&source) {
                    regex.push_str(&regex::escape(&source[end..field.start()]));
                    regex.push_str("(.*?)");
                    end = field.end();
                }
                regex.push_str(&regex::escape(&source[end..]));
                regex.push('$');
                Pattern {
                    source,
                    regex: Regex::new(&regex).unwrap(),
                }
            })
            .collect()
    })
}
fn interpolate(template: &str, values: &[&str]) -> String {
    let mut output = String::new();
    let mut remaining = template;
    while let Some(start) = remaining.find('{') {
        output.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        if let Some(end) = remaining.find('}')
            && let Ok(index) = remaining[1..end].parse::<usize>()
            && let Some(value) = values.get(index)
        {
            output.push_str(value);
            remaining = &remaining[end + 1..];
        } else {
            output.push('{');
            remaining = &remaining[1..];
        }
    }
    output.push_str(remaining);
    output
}
pub fn translate_message(source: &str) -> String {
    let translated = native_text(source);
    if translated != source {
        return translated.to_owned();
    }
    if CURRENT.load(Ordering::Relaxed) == 0 {
        return source.to_owned();
    }
    for pattern in patterns() {
        if let Some(captures) = pattern.regex.captures(source) {
            let template = native_text(&pattern.source);
            if template == pattern.source {
                continue;
            }
            let values: Vec<&str> = captures
                .iter()
                .skip(1)
                .map(|c| c.unwrap().as_str())
                .collect();
            return interpolate(template, &values);
        }
    }
    if source.contains('\n') {
        return source
            .split('\n')
            .map(translate_message)
            .collect::<Vec<_>>()
            .join("\n");
    }
    source.to_owned()
}

fn dictionary(data: &'static str) -> &'static HashMap<String, String> {
    static DICTIONARIES: OnceLock<HashMap<&'static str, HashMap<String, String>>> = OnceLock::new();
    DICTIONARIES
        .get_or_init(|| {
            [
                include_str!("../locales/properties.json"),
                include_str!("../locales/widgets.json"),
                include_str!("../locales/enums.json"),
            ]
            .into_iter()
            .map(|json| (json, serde_json::from_str(json).unwrap()))
            .collect()
        })
        .get(data)
        .unwrap()
}
pub fn property_label(key: &str) -> String {
    dictionary(include_str!("../locales/properties.json"))
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.replace('_', " "))
}
pub fn widget_label(key: &str) -> String {
    dictionary(include_str!("../locales/widgets.json"))
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.replace('_', " "))
}
pub fn enum_label(key: &str) -> String {
    dictionary(include_str!("../locales/enums.json"))
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.replace('_', " "))
}

pub fn resolve(locale: &str) -> usize {
    let normalized = locale
        .split(['.', '@'])
        .next()
        .unwrap_or(locale)
        .replace('_', "-")
        .to_lowercase();
    if let Some(index) = LANGUAGES
        .iter()
        .position(|l| l.code.to_lowercase() == normalized)
    {
        return index;
    }
    let parts: Vec<&str> = normalized.split('-').collect();
    let code = match parts.first().copied().unwrap_or("") {
        "zh" => {
            if parts
                .iter()
                .any(|p| matches!(*p, "hant" | "tw" | "hk" | "mo"))
            {
                "zh-Hant"
            } else {
                "zh-Hans"
            }
        }
        "pt" => {
            if parts.contains(&"br") {
                "pt-BR"
            } else {
                "pt-PT"
            }
        }
        "no" | "nb" => "nb",
        "iw" => "he",
        "in" => "id",
        code => code,
    };
    LANGUAGES.iter().position(|l| l.code == code).unwrap_or(0)
}
pub fn chosen(language: Option<&str>, system: &str) -> usize {
    resolve(language.unwrap_or(system))
}
pub fn activate(language: Option<&str>) -> bool {
    let system = sys_locale::get_locale().unwrap_or_else(|| "en".into());
    let index = chosen(language, &system);
    CURRENT.swap(index, Ordering::Relaxed) != index
}
fn search_key(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect()
}
fn matches(language: &Language, query: &str) -> bool {
    search_key(&format!(
        "{} {} {}",
        language.native, language.english, language.code
    ))
    .contains(&search_key(query))
}
pub struct Interface {
    pub visible: bool,
    query: String,
    was_open: bool,
    last_check: Instant,
}
impl Default for Interface {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            was_open: false,
            last_check: Instant::now() - Duration::from_secs(2),
        }
    }
}
impl Interface {
    pub fn refresh(&mut self, prefs: &crate::prefs::Prefs, ctx: &egui::Context) -> bool {
        if prefs.language.is_some() {
            return false;
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return false;
        }
        self.last_check = Instant::now();
        activate(None)
    }
    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut crate::prefs::Prefs) -> bool {
        if !self.visible {
            return false;
        }
        let mut visible = true;
        let mut changed = false;
        egui::Window::new(text("Interface")).open(&mut visible).default_width(480.0).show(ctx, |ui| {
            ui.label(ui_text(ui, "Language"));
            let selected = prefs.language.as_deref().map(|code| LANGUAGES[resolve(code)].native).unwrap_or(native_text("System default"));
            let response = egui::ComboBox::from_id_salt("interface-language").width(340.0).height(300.0).selected_text(visual_text(selected)).show_ui(ui, |ui| {
                if !self.was_open { self.query.clear(); }
                let search = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(text("Search languages…")));
                if !self.was_open { search.request_focus(); }
                ui.separator();
                // Always the first option, including while searching.
                if ui.selectable_value(&mut prefs.language, None, text("System default")).changed() { changed = true; ui.close(); }
                let mut found = false;
                for language in LANGUAGES.iter().filter(|l| matches(l, &self.query)) {
                    found = true;
                    let label = visual_text(&format!("{} — {}", language.native, language.english));
                    if ui.selectable_value(&mut prefs.language, Some(language.code.to_owned()), label).changed() { changed = true; ui.close(); }
                }
                if !found { ui.weak(text("No languages found")); }
            });
            self.was_open = response.inner.is_some();
            ui.small(ui_text(ui, "System default follows your system language. Unsupported languages use English."));
            ui.separator();
            changed |= ui.checkbox(&mut prefs.show_diagnostic_data, text("Show diagnostic data")).changed();
            ui.small(ui_text(ui, "Show decoder, frame, audio synchronization and rendering statistics below the player."));
        });
        self.visible = visible;
        changed
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let variants: &[(&str, &[u8])] = &[
        (
            "sc",
            include_bytes!("../../../assets/fonts/interface/NotoSans-sc.ttf"),
        ),
        (
            "tc",
            include_bytes!("../../../assets/fonts/interface/NotoSans-tc.ttf"),
        ),
        (
            "jp",
            include_bytes!("../../../assets/fonts/interface/NotoSans-jp.ttf"),
        ),
        (
            "kr",
            include_bytes!("../../../assets/fonts/interface/NotoSans-kr.ttf"),
        ),
        (
            "ar",
            include_bytes!("../../../assets/fonts/interface/NotoSans-ar.ttf"),
        ),
        (
            "he",
            include_bytes!("../../../assets/fonts/interface/NotoSans-he.ttf"),
        ),
    ];
    let code = LANGUAGES[CURRENT.load(Ordering::Relaxed)].code;
    let preferred = match code {
        "ja" => "jp",
        "ko" => "kr",
        "zh-Hant" => "tc",
        _ => "sc",
    };
    let order = std::iter::once(preferred).chain(
        variants
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| *id != preferred),
    );
    for id in order {
        let data = variants.iter().find(|(name, _)| *name == id).unwrap().1;
        let name = format!("ActionLay interface {id}");
        fonts.font_data.insert(
            name.clone(),
            std::sync::Arc::new(egui::FontData::from_static(data)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn regional_settings(
    ctx: &egui::Context,
    visible: &mut bool,
    prefs: &mut crate::prefs::Prefs,
) -> bool {
    if !*visible {
        return false;
    }
    use actionlay_layout::model::Units;
    let mut changed = false;
    egui::Window::new(text("Regional settings")).open(visible).default_width(420.0).show(ctx, |ui| {
        ui.label(text("Measurement units"));
        egui::ComboBox::from_id_salt("regional-units").selected_text(text(match prefs.regional_units {
            Some(Units::Metric) => "Metric units", Some(Units::Imperial) => "Imperial units", _ => "System default",
        })).show_ui(ui, |ui| {
            for (value, label) in [(None, "System default"), (Some(Units::Metric), "Metric units"), (Some(Units::Imperial), "Imperial units")] {
                changed |= ui.selectable_value(&mut prefs.regional_units, value, text(label)).changed();
            }
        });
        ui.small(ui_text(ui, "Applies to layouts and widgets using Default. Explicit units keep their own setting."));
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_resolution_and_system_default_follow_changes() {
        for (locale, expected) in [
            ("it_IT.UTF-8", "it"),
            ("en-GB", "en"),
            ("zh-Hant-HK", "zh-Hant"),
            ("zh_CN", "zh-Hans"),
            ("pt_BR", "pt-BR"),
            ("pt_PT", "pt-PT"),
            ("no_NO", "nb"),
            ("iw-IL", "he"),
            ("zz-ZZ", "en"),
        ] {
            assert_eq!(LANGUAGES[resolve(locale)].code, expected);
        }
        assert_eq!(LANGUAGES[chosen(None, "it-IT")].code, "it");
        assert_eq!(LANGUAGES[chosen(None, "fr-FR")].code, "fr");
        assert_eq!(LANGUAGES[chosen(Some("de"), "fr-FR")].code, "de");
        let prefs: crate::prefs::Prefs = serde_json::from_str("{}").unwrap();
        assert!(prefs.language.is_none());
        assert!(prefs.regional_units.is_none());
        let saved = serde_json::to_string(&prefs).unwrap();
        assert!(!saved.contains("language"));
        assert!(!saved.contains("regional_units"));
    }
    #[test]
    fn every_catalogue_covers_messages_and_preserves_format_fields() {
        let fields = Regex::new(r"\{\d+\}").unwrap();
        let templates: Vec<String> =
            serde_json::from_str(include_str!("../locales/formatted.json")).unwrap();
        let english = &catalogues()[0].logical;
        for (language, catalogue) in LANGUAGES.iter().zip(catalogues()) {
            for key in english.keys() {
                assert!(
                    catalogue
                        .logical
                        .get(key)
                        .is_some_and(|value| !value.trim().is_empty()),
                    "{} missing {key}",
                    language.code
                );
            }
            for source in &templates {
                let mut expected = fields
                    .find_iter(&english[source])
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>();
                let mut actual = fields
                    .find_iter(&catalogue.logical[source])
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>();
                expected.sort_unstable();
                actual.sort_unstable();
                assert_eq!(actual, expected, "{}: {source}", language.code);
            }
        }
    }
    #[test]
    fn search_matches_native_names_english_names_and_codes() {
        assert!(matches(&LANGUAGES[resolve("fr")], "francais"));
        assert!(matches(&LANGUAGES[resolve("ja")], "日本"));
        assert!(matches(&LANGUAGES[resolve("de")], "german"));
        assert!(matches(&LANGUAGES[resolve("pt-BR")], "pt-br"));
    }
    #[test]
    fn translated_values_do_not_reinterpret_user_braces() {
        assert_eq!(
            interpolate("{1}: {0}", &["file {1}.mp4", "Error"]),
            "Error: file {1}.mp4"
        );
        assert!(patterns().iter().any(|p| p.source == "About {} remaining"
            && p.regex.is_match("About 1 h 2 min 3 s remaining")));
        assert_ne!(visual_text("العربية"), "العربية");
        assert_eq!(visual_text("ActionLay 1.0.1"), "ActionLay 1.0.1");
    }
    #[test]
    fn interface_fonts_render_all_language_names_and_settings() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for (language, catalogue) in LANGUAGES.iter().zip(catalogues()) {
                let label = format!(
                    "{} — {}",
                    language.native, catalogue.logical["Regional settings"]
                );
                let galley = ui.fonts_mut(|fonts| {
                    fonts.layout_no_wrap(
                        visual_text(&label),
                        egui::FontId::proportional(16.0),
                        egui::Color32::WHITE,
                    )
                });
                assert!(
                    galley.size().x.is_finite() && galley.size().x > 0.0,
                    "{}",
                    language.code
                );
                ui.label(galley);
            }
        });
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
    }
}
