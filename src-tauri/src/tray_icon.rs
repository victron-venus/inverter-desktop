use ab_glyph::{FontRef, PxScale};
use image::{ImageEncoder, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;
use std::sync::atomic::{AtomicBool, Ordering};

// ── Font (SF Pro Rounded ≈ bold) ────────────────────────────────
//const FONT_DATA: &[u8] = include_bytes!("/System/Library/Fonts/SFNSRounded.ttf");
//const FONT_DATA: &[u8] = include_bytes!("/System/Library/Fonts/HelveticaNeue.ttc");
const FONT_DATA: &[u8] = include_bytes!("/System/Library/Fonts/Supplemental/Arial Narrow Bold.ttf");

// ── Breathing toggle ────────────────────────────────────────────
static NEXT_DIM: AtomicBool = AtomicBool::new(true);

// ── Canvas @2x (166 × 42 logical pts → 332 × 84px) ─────────────
const W: u32 = 332;
const H: u32 = 84;

// ── Layout ──────────────────────────────────────────────────────
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

// 7*17 + 6*4 = 143 → BAR_X + 143 = 330 → right margin = 332-330 = 2
// 2*38 + 4 = 80 → 4px bottom padding
const ROW_YS: [i32; 2] = [0, 42];

const INACTIVE: Rgba<u8> = Rgba([200, 200, 200, 50]);
const SOLAR_RGB: (u8, u8, u8) = (255, 245, 100);
const GRID_IN_RGB: (u8, u8, u8) = (150, 220, 255);
const GRID_OUT_RGB: (u8, u8, u8) = (255, 130, 255);
const TEXT_COLOR: Rgba<u8> = Rgba([255, 255, 255, 255]);

fn seg_x(seg: u32) -> i32 {
    BAR_X + seg as i32 * (SEG_W + SEG_GAP) as i32
}

fn with_alpha(rgb: (u8, u8, u8), a: u8) -> Rgba<u8> {
    Rgba([rgb.0, rgb.1, rgb.2, a])
}

fn draw_seg(img: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>, top_row: bool) {
    let w = SEG_W as i32;
    let h = SEG_H as i32;
    let r = CORNER;

    if top_row {
        draw_filled_rect_mut(img, Rect::at(x, y + r).of_size(SEG_W, (h - r) as u32), color);
        draw_filled_circle_mut(img, (x + r, y + r), r, color);
        draw_filled_circle_mut(img, (x + w - 1 - r, y + r), r, color);
    } else {
        draw_filled_rect_mut(img, Rect::at(x, y).of_size(SEG_W, (h - r) as u32), color);
        let by = y + h - 1;
        draw_filled_circle_mut(img, (x + r, by - r), r, color);
        draw_filled_circle_mut(img, (x + w - 1 - r, by - r), r, color);
    }
}

// ── Smart value formatting ──────────────────────────────────────
fn fmt_solar(val: Option<f64>) -> String {
    match val {
        Some(v) if v > 0.0 => {
            if v < 1000.0 {
                format!("{:.0}", v)
            } else {
                format!("{:.1}k", v / 1000.0)
            }
        }
        _ => "0".into(),
    }
}

fn fmt_grid(val: Option<f64>) -> String {
    match val {
        Some(v) => {
            let a = v.abs();
            if a < 1000.0 {
                format!("{:+.0}", v)
            } else {
                format!("{:+.1}k", v / 1000.0)
            }
        }
        None => "0".into(),
    }
}

// ── Log-scale level ─────────────────────────────────────────────
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

const MAX_SOLAR: f64 = 10_000.0;
const MAX_GRID: f64 = 5_000.0;

// ── Public ──────────────────────────────────────────────────────
pub fn render(solar_total: Option<f64>, grid_power: Option<f64>) -> Vec<u8> {
    let font = FontRef::try_from_slice(FONT_DATA).expect("SF Rounded");

    // ── Breathing ──────────────────────────────────────────────
    let prev = NEXT_DIM.load(Ordering::Relaxed);
    NEXT_DIM.store(!prev, Ordering::Relaxed);
    let alpha: u8 = if prev { 170 } else { 200 };

    let solar = with_alpha(SOLAR_RGB, alpha);
    let grid_active = match grid_power {
        Some(v) if v > 0.0 => with_alpha(GRID_IN_RGB, alpha),
        _ => with_alpha(GRID_OUT_RGB, alpha),
    };
    let row_colors = [solar, grid_active];

    // ── Text labels ────────────────────────────────────────────
    let labels = [fmt_solar(solar_total), fmt_grid(grid_power)];

    // ── Render ─────────────────────────────────────────────────
    let mut img = RgbaImage::new(W, H);

    let levels = [level(solar_total, MAX_SOLAR), level(grid_power, MAX_GRID)];

    for row in 0..N_ROWS {
        let y = ROW_YS[row as usize];
        let lv = levels[row as usize];
        let color = row_colors[row as usize];

        for seg in 0..N_SEG {
            let x = seg_x(seg);
            let fill = if seg < lv { color } else { INACTIVE };
            draw_seg(&mut img, x, y, fill, row == 0);
        }

        let label = &labels[row as usize];
        let (tw, _th) = text_size(FONT_SCALE, &font, label);
        let tx = TEXT_X + TEXT_W - tw as i32 - 19;
        let ty = y - 4;
        draw_text_mut(&mut img, TEXT_COLOR, tx, ty, FONT_SCALE, &font, label);
    }

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(img.as_raw(), W, H, image::ExtendedColorType::Rgba8)
        .expect("tray icon PNG encode failed");
    png
}
