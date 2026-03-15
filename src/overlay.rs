use crate::ipc;
use crate::paths::AppPaths;
use anyhow::{Context, Result};
use eframe::egui::{
    self, Color32, CornerRadius, Frame, Margin, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
    Vec2, ViewportCommand,
};
use std::process::Child;
use std::time::{Duration, Instant};

const WINDOW_WIDTH: f32 = 94.0;
const WINDOW_HEIGHT: f32 = 28.0;
const WINDOW_MARGIN_BOTTOM: f32 = 36.0;
const VISUAL_BAR_COUNT: usize = 12;
const LEVEL_SAMPLE_MS: u64 = 32;

pub fn run(paths: AppPaths) -> Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("hermes-overlay")
        .with_decorations(false)
        .with_transparent(true)
        .with_active(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_resizable(false)
        .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
        .with_min_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
        .with_max_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT]);

    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some() {
        viewport = viewport.with_window_type(egui::X11WindowType::Tooltip);
    }

    let mut native_options = eframe::NativeOptions {
        viewport,
        persist_window: false,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some() {
        native_options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }));
    }

    eframe::run_native(
        "Hermes overlay",
        native_options,
        Box::new(|_| Ok(Box::new(OverlayApp::new(paths)))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub fn spawn() -> Result<Child> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    std::process::Command::new(exe)
        .arg("overlay")
        .spawn()
        .context("failed to launch overlay process")
}

struct OverlayApp {
    paths: AppPaths,
    smoothed_level: f32,
    peak_level: f32,
    level_history: [f32; VISUAL_BAR_COUNT],
    last_level_sample_at: Instant,
    positioned: bool,
}

impl OverlayApp {
    fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            smoothed_level: 0.0,
            peak_level: 0.0,
            level_history: [0.0; VISUAL_BAR_COUNT],
            last_level_sample_at: Instant::now(),
            positioned: false,
        }
    }

    fn update_position(&mut self, ctx: &egui::Context) {
        if self.positioned {
            return;
        }

        let Some(monitor_size) = ctx.input(|input| input.viewport().monitor_size) else {
            return;
        };

        let x = ((monitor_size.x - WINDOW_WIDTH) / 2.0).max(0.0);
        let y = (monitor_size.y - WINDOW_HEIGHT - WINDOW_MARGIN_BOTTOM).max(0.0);
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(x, y)));
        self.positioned = true;
    }

    fn sample_level_history(&mut self, level: f32) {
        if self.last_level_sample_at.elapsed() < Duration::from_millis(LEVEL_SAMPLE_MS) {
            return;
        }

        self.level_history.rotate_right(1);
        self.level_history[0] = level.clamp(0.0, 1.0);
        self.last_level_sample_at = Instant::now();
    }
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(33));

        if !ipc::heartbeat_is_fresh(&self.paths, Duration::from_secs(3)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        self.update_position(ctx);

        let live_level = ipc::read_audio_level(&self.paths).clamp(0.0, 1.0);
        let visual_level = overlay_visual_level(live_level);
        self.smoothed_level = self.smoothed_level * 0.58 + visual_level * 0.42;
        self.peak_level = (self.peak_level * 0.92).max(self.smoothed_level);
        self.sample_level_history(self.smoothed_level.max(visual_level * 0.96));

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .inner_margin(Margin::same(0))
                    .fill(Color32::TRANSPARENT),
            )
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                paint_overlay(ui.painter(), rect, self.peak_level, &self.level_history);
            });
    }
}

fn overlay_visual_level(raw_level: f32) -> f32 {
    let boosted = (raw_level * 5.4).clamp(0.0, 1.0);
    let curved = boosted.powf(0.56);
    if curved < 0.02 { 0.0 } else { curved }
}

fn paint_overlay(
    painter: &Painter,
    rect: Rect,
    peak_level: f32,
    level_history: &[f32; VISUAL_BAR_COUNT],
) {
    let bg = Color32::from_rgba_unmultiplied(0, 0, 0, 232);
    let accent = Color32::from_rgba_unmultiplied(255, 255, 255, 248);
    let border = Color32::from_rgba_unmultiplied(255, 255, 255, 46);

    let pill = rect.shrink2(Vec2::new(2.0, 1.0));
    let radius = CornerRadius::same(((pill.height() * 0.5).ceil() as i32).clamp(1, 255) as u8);
    painter.rect(
        pill,
        radius,
        bg,
        Stroke::new(1.55, border),
        StrokeKind::Outside,
    );

    let bars_rect = Rect::from_center_size(
        Pos2::new(pill.center().x, pill.center().y),
        Vec2::new(pill.width() - 16.0, pill.height() - 9.0),
    );
    paint_wave_bars(painter, bars_rect, level_history, peak_level, accent);
}

fn paint_wave_bars(
    painter: &Painter,
    rect: Rect,
    level_history: &[f32; VISUAL_BAR_COUNT],
    peak_level: f32,
    color: Color32,
) {
    let bar_width = 3.2;
    let gap = 2.0;
    let total_width =
        VISUAL_BAR_COUNT as f32 * bar_width + (VISUAL_BAR_COUNT.saturating_sub(1) as f32) * gap;
    let left = rect.center().x - total_width * 0.5;
    let center_y = rect.center().y;
    let mid = (VISUAL_BAR_COUNT as f32 - 1.0) / 2.0;

    for index in 0..VISUAL_BAR_COUNT {
        let distance = (index as f32 - mid).abs() / mid.max(1.0);
        let envelope = (1.0 - distance.powf(1.45)).max(0.22);
        let history_value = level_history[index.min(level_history.len() - 1)];
        let bar_level = (history_value * 0.8 + peak_level * 0.2).clamp(0.0, 1.0);
        let height = (3.6 + envelope * 3.2 + bar_level * envelope * 23.0).clamp(3.6, rect.height());
        let bar = Rect::from_center_size(
            Pos2::new(
                left + index as f32 * (bar_width + gap) + bar_width * 0.5,
                center_y,
            ),
            Vec2::new(bar_width, height),
        );
        painter.add(Shape::rect_filled(
            bar,
            CornerRadius::same(2),
            color.gamma_multiply(0.38 + envelope * 0.62),
        ));
    }
}
