//! Window geometry and the player's temporary full-screen presentation.
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    /// Physical desktop coordinates; unlike dimensions these must not be scaled
    /// again when restoring a window on a different monitor.
    pub position: Option<[i32; 2]>,
    pub size: [f32; 2],
    pub maximized: bool,
}

const HIDE_AFTER: Duration = Duration::from_secs(3);
const SETTLE: Duration = Duration::from_millis(500);

pub struct State {
    pub fullscreen: bool,
    activity: Instant,
    settle_until: Instant,
    geometry_changed: Option<Instant>,
    restore_maximized: bool,
}

impl State {
    pub fn new(cc: &eframe::CreationContext<'_>, saved: &mut Option<Geometry>) -> Self {
        let now = Instant::now();
        if let Some(window) = cc.winit_window() {
            let monitors: Vec<_> = window.available_monitors().collect();
            let target = saved
                .as_ref()
                .and_then(|g| g.position)
                .and_then(|[x, y]| {
                    monitors.iter().find(|m| {
                        let p = m.position();
                        let s = m.size();
                        x >= p.x
                            && y >= p.y
                            && i64::from(x) < i64::from(p.x) + i64::from(s.width)
                            && i64::from(y) < i64::from(p.y) + i64::from(s.height)
                    })
                })
                .cloned()
                .or_else(|| window.current_monitor())
                .or_else(|| window.primary_monitor());
            if let Some(monitor) = target {
                let scale = monitor.scale_factor();
                let m = monitor.size();
                let origin = monitor.position();
                let (size, position) = placement(
                    [origin.x, origin.y],
                    [m.width, m.height],
                    scale,
                    saved.as_ref(),
                );
                let mut physical_size = monitor.size();
                physical_size.width = size[0];
                physical_size.height = size[1];
                let mut minimum = physical_size;
                minimum.width = minimum.width.min((720.0 * scale) as u32);
                minimum.height = minimum.height.min((480.0 * scale) as u32);
                window.set_min_inner_size(Some(minimum));
                let _ = window.request_inner_size(physical_size);
                let mut physical_position = monitor.position();
                physical_position.x = position[0];
                physical_position.y = position[1];
                window.set_outer_position(physical_position);
                *saved = Some(Geometry {
                    position: Some(position),
                    size: [size[0] as f32 / scale as f32, size[1] as f32 / scale as f32],
                    maximized: saved.as_ref().is_some_and(|g| g.maximized),
                });
            }
            window.set_maximized(saved.as_ref().is_some_and(|g| g.maximized));
        }
        Self {
            fullscreen: false,
            activity: now,
            settle_until: now + SETTLE,
            geometry_changed: None,
            restore_maximized: saved.as_ref().is_some_and(|g| g.maximized),
        }
    }

    pub fn set_fullscreen(&mut self, ctx: &egui::Context, enabled: bool) {
        if self.fullscreen == enabled {
            return;
        }
        if enabled {
            self.restore_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        }
        self.fullscreen = enabled;
        self.activity = Instant::now();
        self.settle_until = self.activity + SETTLE;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(enabled));
        if !enabled {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.restore_maximized));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        }
        ctx.request_repaint();
    }

    /// Native full-screen buttons use the same player presentation. Wait for
    /// our requested transition before interpreting OS feedback as a new action.
    pub fn sync_fullscreen(&mut self, frame: &eframe::Frame, ctx: &egui::Context, allowed: bool) {
        if Instant::now() < self.settle_until {
            return;
        }
        if let Some(window) = frame.winit_window() {
            let actual = window.fullscreen().is_some();
            if actual && allowed && !self.fullscreen {
                self.fullscreen = true;
                self.activity = Instant::now();
                ctx.request_repaint();
            } else if !actual && self.fullscreen {
                self.set_fullscreen(ctx, false);
            }
        }
    }

    pub fn controls_visible(&mut self, ctx: &egui::Context, dragging: bool) -> bool {
        let now = Instant::now();
        if dragging
            || ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::PointerMoved(_)
                            | egui::Event::PointerButton { .. }
                            | egui::Event::MouseWheel { .. }
                    )
                })
            })
        {
            self.activity = now;
        }
        let visible = controls_visible(self.activity, now);
        if visible {
            ctx.request_repaint_after(HIDE_AFTER.saturating_sub(now - self.activity));
        }
        visible
    }

    /// Keep normal bounds while maximized, minimized or full-screen. Debounce
    /// disk writes, but update preferences immediately so closing saves them.
    pub fn capture(&mut self, frame: &eframe::Frame, saved: &mut Option<Geometry>) -> bool {
        let now = Instant::now();
        if now < self.settle_until || self.fullscreen {
            return false;
        }
        if let Some(window) = frame.winit_window()
            && window.fullscreen().is_none()
            && window.is_minimized() != Some(true)
        {
            let maximized = window.is_maximized();
            self.restore_maximized = maximized;
            let size = window.inner_size().to_logical::<f32>(window.scale_factor());
            let next = if maximized {
                saved.as_ref().map(|g| Geometry {
                    maximized: true,
                    ..g.clone()
                })
            } else if size.width > 0.0 && size.height > 0.0 {
                Some(Geometry {
                    position: window.outer_position().ok().map(|p| [p.x, p.y]),
                    size: [size.width, size.height],
                    maximized: false,
                })
            } else {
                None
            };
            if next.is_some() && *saved != next {
                *saved = next;
                self.geometry_changed = Some(now);
            }
        }
        if self.geometry_changed.is_some_and(|at| now - at >= SETTLE) {
            self.geometry_changed = None;
            true
        } else {
            false
        }
    }
}

fn controls_visible(activity: Instant, now: Instant) -> bool {
    now - activity < HIDE_AFTER
}

/// Leave space for the title bar, taskbar and screen edges. Removed monitors
/// and invalid saved dimensions fall back to a centered, comfortable window.
fn placement(
    origin: [i32; 2],
    monitor: [u32; 2],
    scale: f64,
    saved: Option<&Geometry>,
) -> ([u32; 2], [i32; 2]) {
    let default_max = [monitor[0] as f64 * 0.85, monitor[1] as f64 * 0.85];
    let max = if saved.is_some() {
        [
            (monitor[0] as f64 - 48.0 * scale).max(1.0),
            (monitor[1] as f64 - 72.0 * scale).max(1.0),
        ]
    } else {
        default_max
    };
    let default = [
        1200.0_f64.min(default_max[0] / scale),
        800.0_f64.min(default_max[1] / scale),
    ];
    let logical = saved
        .map(|g| g.size.map(f64::from))
        .filter(|s| s.iter().all(|v| v.is_finite() && *v >= 100.0))
        .unwrap_or(default);
    let size = [
        ((logical[0] * scale).min(max[0]).max(1.0)) as u32,
        ((logical[1] * scale).min(max[1]).max(1.0)) as u32,
    ];
    let margin = (24.0 * scale) as i32;
    let center = [
        origin[0] + (monitor[0] - size[0]) as i32 / 2,
        origin[1] + (monitor[1] - size[1]) as i32 / 2,
    ];
    let position = saved
        .and_then(|g| g.position)
        .filter(|p| {
            (0..2).all(|i| {
                p[i] >= origin[i] && i64::from(p[i]) < i64::from(origin[i]) + i64::from(monitor[i])
            })
        })
        .unwrap_or(center);
    let position = std::array::from_fn(|i| {
        position[i].clamp(
            origin[i] + margin,
            (origin[i] + monitor[i] as i32 - size[i] as i32 - margin * 2).max(origin[i] + margin),
        )
    });
    (size, position)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_fit_small_high_dpi_screens_and_removed_monitors() {
        let (size, pos) = placement([0, 0], [1920, 1080], 1.5, None);
        assert!(size[1] < 1080 - 100);
        assert_eq!(
            pos,
            [(1920 - size[0]) as i32 / 2, (1080 - size[1]) as i32 / 2]
        );
        let saved = Geometry {
            position: Some([-2000, 50]),
            size: [4000.0, 3000.0],
            maximized: true,
        };
        let (size, pos) = placement([0, 0], [1366, 768], 1.0, Some(&saved));
        assert!(pos[0] >= 0 && pos[1] > 0);
        assert!(pos[1] + (size[1] as i32) < 768);
        let (size, pos) = placement(
            [-1920, 0],
            [1920, 1080],
            1.0,
            Some(&Geometry {
                position: Some([-1800, 100]),
                size: [900.0, 600.0],
                maximized: false,
            }),
        );
        assert_eq!(size, [900, 600]);
        assert_eq!(pos, [-1800, 100]);
    }
    #[test]
    fn full_screen_controls_hide_after_three_seconds_and_movement_restarts_timer() {
        let start = Instant::now();
        let ctx = egui::Context::default();
        let mut state = State {
            fullscreen: true,
            activity: start - Duration::from_secs(4),
            settle_until: start,
            geometry_changed: None,
            restore_maximized: false,
        };
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let ctx = ui.ctx();
            assert!(!state.controls_visible(ctx, false));
        });
        output.textures_delta.clear();
        let input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(egui::pos2(100.0, 100.0))],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx();
            assert!(state.controls_visible(ctx, false));
        });
        output.textures_delta.clear();
        state.activity = Instant::now() - HIDE_AFTER;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let ctx = ui.ctx();
            assert!(!state.controls_visible(ctx, false));
            // A seek drag must remain usable even when held still.
            assert!(state.controls_visible(ctx, true));
            state.set_fullscreen(ctx, false);
        });
        output.textures_delta.clear();
        assert!(!state.fullscreen);
    }
}
