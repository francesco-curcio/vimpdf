use std::path::PathBuf;

use eframe::egui::{
    self, Color32, ColorImage, Key, Pos2, Rect, TextureHandle, TextureOptions, Vec2,
};

use crate::document::DocInfo;
use crate::render::{RenderJob, Renderer};

const BACKGROUND: Color32 = Color32::from_gray(40);
const PAGE_MARGIN: f32 = 16.0;

pub struct PdfvApp {
    info: DocInfo,
    renderer: Renderer,
    /// Last finished render of the current page and the scale it was made at.
    texture: Option<(TextureHandle, f32)>,
    /// Scale of the most recently requested render.
    requested_scale: Option<f32>,
    error: Option<String>,
}

impl PdfvApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: PathBuf, info: DocInfo) -> Self {
        let ctx = cc.egui_ctx.clone();
        let renderer = Renderer::spawn(path, move || ctx.request_repaint());
        Self {
            info,
            renderer,
            texture: None,
            requested_scale: None,
            error: None,
        }
    }

    fn receive_renders(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.renderer.try_recv() {
            match result.image {
                Ok(img) => {
                    let image = ColorImage::from_rgb([img.width, img.height], &img.rgb);
                    let handle = ctx.load_texture(
                        format!("page-{}", result.job.page),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.texture = Some((handle, result.job.scale));
                    self.error = None;
                }
                Err(err) => {
                    tracing::error!("render failed: {err}");
                    self.error = Some(err);
                }
            }
        }
    }
}

/// Round a scale so tiny size changes reuse the same render.
fn bucket_scale(scale: f32) -> f32 {
    (scale * 100.0).round() / 100.0
}

/// Move a rect so its top-left corner sits on a physical pixel, keeping its size.
fn snap_to_pixels(rect: Rect, ppp: f32) -> Rect {
    let min = Pos2::new(
        (rect.min.x * ppp).round() / ppp,
        (rect.min.y * ppp).round() / ppp,
    );
    Rect::from_min_size(min, rect.size())
}

impl eframe::App for PdfvApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.key_pressed(Key::Q)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        self.receive_renders(&ctx);

        let area = ui.max_rect();
        ui.painter().rect_filled(area, 0.0, BACKGROUND);

        // M0: show page 1 fitted to the window.
        let page = 0;
        let size = self.info.page_sizes[page];
        let avail = (area.size() - Vec2::splat(2.0 * PAGE_MARGIN)).max(Vec2::splat(1.0));
        let zoom = (avail.x / size.width).min(avail.y / size.height);
        let ppp = ctx.pixels_per_point();
        let scale = bucket_scale(zoom * ppp);
        if self.requested_scale != Some(scale) {
            self.renderer.request(RenderJob { page, scale });
            self.requested_scale = Some(scale);
        }

        let painter = ui.painter();
        match &self.texture {
            Some((tex, tex_scale)) => {
                // If the texture matches the current scale, draw it 1:1 on the
                // physical pixel grid so it is not resampled. Otherwise draw the
                // stale texture stretched until the sharper render arrives.
                let page_size = if *tex_scale == scale {
                    tex.size_vec2() / ppp
                } else {
                    Vec2::new(size.width, size.height) * zoom
                };
                let page_rect =
                    snap_to_pixels(Rect::from_center_size(area.center(), page_size), ppp);
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                painter.image(tex.id(), page_rect, uv, Color32::WHITE);
            }
            None => {
                let page_size = Vec2::new(size.width, size.height) * zoom;
                let page_rect = Rect::from_center_size(area.center(), page_size);
                painter.rect_filled(page_rect, 0.0, Color32::WHITE);
            }
        }

        if let Some(err) = &self.error {
            painter.text(
                area.left_bottom() + Vec2::new(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                err,
                egui::FontId::proportional(14.0),
                Color32::LIGHT_RED,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_scale_rounds_to_two_decimals() {
        assert_eq!(bucket_scale(2.0049), 2.0);
        assert_eq!(bucket_scale(2.006), 2.01);
    }

    #[test]
    fn snap_to_pixels_aligns_on_retina() {
        let r = snap_to_pixels(
            Rect::from_min_size(Pos2::new(10.3, 5.8), Vec2::splat(4.0)),
            2.0,
        );
        assert_eq!(r.min, Pos2::new(10.5, 6.0));
        assert_eq!(r.size(), Vec2::splat(4.0));
    }
}
