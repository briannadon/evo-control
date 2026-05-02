//! Custom egui widgets for evo-control.

use eframe::egui::{self, Color32, CornerRadius, Sense, Vec2};

/// A toggle button with a filled/empty indicator lamp.
/// `on` = filled color; `off` = dim outline. Click toggles.
pub fn toggle_lamp(ui: &mut egui::Ui, label: &str, on: bool, color: Color32) -> bool {
    let (fill, text) = if on {
        (color, color)
    } else {
        (Color32::DARK_GRAY.gamma_multiply(0.4), Color32::GRAY)
    };

    let desired = Vec2::new(60.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);

        // Background pill
        painter.rect_filled(rect, CornerRadius::same(6), fill);

        // Label
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(11.0),
            text,
        );
    }

    response.clicked()
}

/// A dB-aware vertical slider. Returns the clamped dB value if changed.
pub fn level_slider(ui: &mut egui::Ui, label: &str, db: &mut f32, min: f32, max: f32) -> bool {
    let mut changed = false;

    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).size(10.0).color(Color32::GRAY));
        let slider = egui::Slider::new(db, min..=max)
            .text("")
            .show_value(false)
            .orientation(egui::SliderOrientation::Vertical)
            .clamping(egui::SliderClamping::Always)
            .smart_aim(false);
        let resp = ui.add(slider);
        if resp.dragged() || resp.changed() {
            changed = true;
        }
        ui.label(format!("{db:.0}"));
    });

    changed
}

/// A rotary knob drawn with the egui painter.
///
/// `curve` controls the mapping from knob angle to value:
/// - `curve = 1.0` → linear (equal angle = equal value change)
/// - `curve < 1.0` → audio taper (more value at low angles, finer control at the top)
///   This is the standard for gain/volume knobs: `value = min + (max-min) * pos^curve`.
///
/// Returns `true` if the value was changed by user interaction.
pub fn rotary_knob(ui: &mut egui::Ui, value: &mut f32, min: f32, max: f32, size: f32, curve: f32) -> bool {
    fn linear_t(value: f32, min: f32, max: f32) -> f32 {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }

    let range = max - min;
    let t = linear_t(*value, min, max);
    // Knob position that produces this value under the curve.
    let pos = t.powf(1.0 / curve);

    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let radius = rect.width().min(rect.height()) / 2.0 - 4.0;

        // Background circle
        painter.circle_filled(center, radius, Color32::from_gray(40));
        painter.circle_stroke(center, radius, egui::Stroke::new(1.0, Color32::GRAY));

        // Value arc — angle follows the knob position (pos), not the linear t
        let angle = std::f32::consts::PI * 0.75 + pos * std::f32::consts::PI * 1.5;
        let indicator_radius = radius * 0.7;
        let tip = egui::pos2(
            center.x + angle.cos() * indicator_radius,
            center.y + angle.sin() * indicator_radius,
        );
        painter.circle_filled(tip, 3.0, Color32::WHITE);

        // Arc track
        let arc_start = std::f32::consts::PI * 0.75;
        let arc_end = arc_start + pos * std::f32::consts::PI * 1.5;
        let n_segments = 32;
        let mut points = Vec::with_capacity(n_segments);
        for i in 0..=n_segments {
            let t = i as f32 / n_segments as f32;
            let a = arc_start + t * (arc_end - arc_start);
            points.push(egui::pos2(
                center.x + a.cos() * (radius - 2.0),
                center.y + a.sin() * (radius - 2.0),
            ));
        }
        painter.add(egui::Shape::line(points, egui::Stroke::new(2.0, Color32::LIGHT_BLUE)));
    }

    // Handle drag — operate in knob-position space (linear with angle)
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = response.drag_delta();
        let speed = 1.0 / 300.0; // 300 pixels = full knob travel
        let new_pos = (pos - delta.y * speed).clamp(0.0, 1.0);
        let new_t = new_pos.powf(curve);
        let new = min + new_t * range;
        if (new - *value).abs() > 0.01 {
            *value = new;
        }
    }

    // Handle scroll
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let step = 1.0 / 100.0;
            let new_pos = (pos - scroll * step).clamp(0.0, 1.0);
            let new_t = new_pos.powf(curve);
            *value = min + new_t * range;
        }
    }

    response.dragged() || response.changed()
}
