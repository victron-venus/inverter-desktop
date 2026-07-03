use ab_glyph::{point, Font, FontRef, GlyphId, PxScale, ScaleFont};
use std::sync::atomic::{AtomicBool, Ordering};

const FONT_DATA: &[u8] = include_bytes!("/System/Library/Fonts/Supplemental/Arial Narrow Bold.ttf");

static NEXT_DIM: AtomicBool = AtomicBool::new(true);

pub const W: u32 = 332;
pub const H: u32 = 84;

const TEXT_X: i32 = 2;
const TEXT_W: i32 = 181;
const BAR_X: i32 = 187;
const SEG_W: u32 = 17;
const SEG_H: u32 = 38;
const SEG_GAP: u32 = 4;
const N_SEG: u32 = 7;
const N_ROWS: u32 = 2;
const CORNER: i32 = 5;
const FONT_SCALE: PxScale = PxScale { x: 65.0, y: 50.0 };

const ROW_YS: [i32; 2] = [0, 42];

#[derive(Clone, Copy)]
struct Rgba(u8, u8, u8, u8);

const INACTIVE: Rgba = Rgba(200, 200, 200, 50);
const SOLAR_RGB: (u8, u8, u8) = (255, 245, 100);
const GRID_IN_RGB: (u8, u8, u8) = (150, 220, 255);
const GRID_OUT_RGB: (u8, u8, u8) = (255, 130, 255);
const TEXT_COLOR: Rgba = Rgba(255, 255, 255, 255);

fn seg_x(seg: u32) -> i32 {
    BAR_X + seg as i32 * (SEG_W + SEG_GAP) as i32
}

fn with_alpha(rgb: (u8, u8, u8), a: u8) -> Rgba {
    Rgba(rgb.0, rgb.1, rgb.2, a)
}

fn set_pixel(pixels: &mut [u8], x: i32, y: i32, color: Rgba) {
    if x < 0 || x >= W as i32 || y < 0 || y >= H as i32 {
        return;
    }
    let idx = (y as usize * W as usize + x as usize) * 4;
    pixels[idx] = color.0;
    pixels[idx + 1] = color.1;
    pixels[idx + 2] = color.2;
    pixels[idx + 3] = color.3;
}

fn fill_rect(pixels: &mut [u8], x: i32, y: i32, w: u32, h: u32, color: Rgba) {
    for dy in 0..h {
        let py = y + dy as i32;
        if py < 0 || py >= H as i32 {
            continue;
        }
        for dx in 0..w {
            let px = x + dx as i32;
            if px < 0 || px >= W as i32 {
                continue;
            }
            set_pixel(pixels, px, py, color);
        }
    }
}

fn fill_circle(pixels: &mut [u8], cx: i32, cy: i32, r: i32, color: Rgba) {
    let r2 = r * r;
    for dy in -r..=r {
        let py = cy + dy;
        if py < 0 || py >= H as i32 {
            continue;
        }
        for dx in -r..=r {
            let px = cx + dx;
            if px < 0 || px >= W as i32 {
                continue;
            }
            if dx * dx + dy * dy <= r2 {
                set_pixel(pixels, px, py, color);
            }
        }
    }
}

fn draw_seg(pixels: &mut [u8], x: i32, y: i32, color: Rgba, top_row: bool) {
    let w = SEG_W as i32;
    let h = SEG_H as i32;
    let r = CORNER;

    if top_row {
        fill_rect(pixels, x, y + r, SEG_W, (h - r) as u32, color);
        fill_circle(pixels, x + r, y + r, r, color);
        fill_circle(pixels, x + w - 1 - r, y + r, r, color);
    } else {
        fill_rect(pixels, x, y, SEG_W, (h - r) as u32, color);
        let by = y + h - 1;
        fill_circle(pixels, x + r, by - r, r, color);
        fill_circle(pixels, x + w - 1 - r, by - r, r, color);
    }
}

fn fmt_solar(val: Option<f64>) -> String {
    match val {
        Some(v) if v > 0.0 => {
            if v < 1000.0 {
                format!("{:.0}W", v)
            } else {
                format!("{:.1}kW", v / 1000.0)
            }
        }
        _ => "0W".into(),
    }
}

fn fmt_grid(val: Option<f64>) -> String {
    match val {
        Some(v) => {
            let a = v.abs();
            let s = if v < 0.0 { "-" } else { "" };
            if a < 1000.0 {
                format!("{}{:.0}W", s, a)
            } else {
                format!("{}{:.1}kW", s, a / 1000.0)
            }
        }
        None => "0W".into(),
    }
}

fn level(val: Option<f64>, max: f64) -> u32 {
    match val {
        Some(v) => {
            let a = v.abs();
            if a <= 0.0 {
                return 0;
            }
            let ratio = (a / max).clamp(0.0, 1.0);
            let segs = (1.0 + ratio * 1999.0).log10() * (7.0 / 3.0);
            (segs.round().min(7.0)) as u32
        }
        None => 0,
    }
}

fn layout_text_width(scale: PxScale, font: &FontRef, text: &str) -> f32 {
    let scaled = font.as_scaled(scale);
    let mut w = 0.0;
    let mut prev: Option<GlyphId> = None;
    for c in text.chars() {
        let gid = scaled.glyph_id(c);
        if let Some(prev_id) = prev {
            w += scaled.kern(gid, prev_id);
        }
        prev = Some(gid);
        w += scaled.h_advance(gid);
    }
    w
}

fn draw_text(
    pixels: &mut [u8],
    x: i32,
    y: i32,
    scale: PxScale,
    font: &FontRef,
    text: &str,
    color: Rgba,
) {
    let scaled = font.as_scaled(scale);
    let mut cursor_x = 0.0_f32;
    let mut prev: Option<GlyphId> = None;

    let base_y = y as f32 + scaled.ascent();

    for c in text.chars() {
        let gid = scaled.glyph_id(c);
        if let Some(prev_id) = prev {
            cursor_x += scaled.kern(gid, prev_id);
        }
        prev = Some(gid);

        let glyph = gid.with_scale_and_position(scale, point(cursor_x + x as f32, base_y));
        if let Some(outlined) = scaled.outline_glyph(glyph) {
            let min_x = outlined.px_bounds().min.x;
            let min_y = outlined.px_bounds().min.y;
            outlined.draw(|gx: u32, gy: u32, coverage: f32| {
                if coverage <= 0.0 {
                    return;
                }
                let px = (min_x + gx as f32) as i32;
                let py = (min_y + gy as f32) as i32;
                if px < 0 || px >= W as i32 || py < 0 || py >= H as i32 {
                    return;
                }
                let idx = (py as usize * W as usize + px as usize) * 4;
                let a = (coverage * color.3 as f32) as u8;
                if a == 0 {
                    return;
                }
                pixels[idx] = color.0;
                pixels[idx + 1] = color.1;
                pixels[idx + 2] = color.2;
                pixels[idx + 3] = a;
            });
        }

        cursor_x += scaled.h_advance(gid);
    }
}

const MAX_SOLAR: f64 = 10_000.0;
const MAX_GRID: f64 = 5_000.0;

pub fn render(solar_total: Option<f64>, grid_power: Option<f64>) -> (Vec<u8>, u32, u32) {
    let font = FontRef::try_from_slice(FONT_DATA).expect("Arial Narrow Bold");

    let prev = NEXT_DIM.load(Ordering::Relaxed);
    NEXT_DIM.store(!prev, Ordering::Relaxed);
    let alpha: u8 = if prev { 170 } else { 200 };

    let solar = with_alpha(SOLAR_RGB, alpha);
    let grid_active = match grid_power {
        Some(v) if v > 0.0 => with_alpha(GRID_IN_RGB, alpha),
        _ => with_alpha(GRID_OUT_RGB, alpha),
    };
    let row_colors = [solar, grid_active];

    let labels = [fmt_solar(solar_total), fmt_grid(grid_power)];

    let mut pixels = vec![0u8; (W * H * 4) as usize];

    let levels = [level(solar_total, MAX_SOLAR), level(grid_power, MAX_GRID)];

    for row in 0..N_ROWS {
        let y = ROW_YS[row as usize];
        let lv = levels[row as usize];
        let color = row_colors[row as usize];

        for seg in 0..N_SEG {
            let x = seg_x(seg);
            let fill = if seg < lv { color } else { INACTIVE };
            draw_seg(&mut pixels, x, y, fill, row == 0);
        }

        let label = &labels[row as usize];
        let tw = layout_text_width(FONT_SCALE, &font, label);
        let tx = TEXT_X + TEXT_W - tw as i32 - 19;
        let ty = y - 4;
        draw_text(&mut pixels, tx, ty, FONT_SCALE, &font, label, TEXT_COLOR);
    }

    (pixels, W, H)
}
