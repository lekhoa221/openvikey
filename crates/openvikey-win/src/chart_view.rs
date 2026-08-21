//! Learning-chart view: GDI timeline painting, Vietnamese text alternative,
//! and effective-state overview counting (spec §15.4).
//!
//! Everything in this module belongs to the Settings/idle path. It must never
//! be called from the hook or inject path; chart data is prepared from the
//! read-only [`ChartSnapshot`] only.

use openvikey_core::chart::{ChartMarker, ChartSnapshot, ChartStateBand};
use openvikey_core::types::InputMethod;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreatePen, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
    FF_DONTCARE, FW_NORMAL, FillRect, HDC, HFONT, LineTo, MoveToEx, OUT_DEFAULT_PRECIS, PS_SOLID,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::core::w;

/// Overview counters by effective band (spec §15.4B).
///
/// Counts reflect planner/guard-effective states, never raw stored states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LearningOverview {
    pub observe: usize,
    pub suggest: usize,
    pub auto: usize,
    pub cooldown: usize,
}

impl LearningOverview {
    #[must_use]
    pub fn from_bands(bands: &[ChartStateBand]) -> Self {
        let mut overview = Self::default();
        for band in bands {
            overview.record(*band);
        }
        overview
    }

    pub fn record(&mut self, band: ChartStateBand) {
        match band {
            ChartStateBand::Observe => self.observe += 1,
            ChartStateBand::Suggest => self.suggest += 1,
            ChartStateBand::Auto => self.auto += 1,
            ChartStateBand::Cooldown => self.cooldown += 1,
        }
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.observe + self.suggest + self.auto + self.cooldown
    }

    /// One-line Vietnamese summary for the Learning page.
    #[must_use]
    pub fn vietnamese_line(&self) -> String {
        format!(
            "Tổng quan: Đang quan sát {} · Gợi ý {} · Có thể tự sửa {} · Tạm dừng tự sửa {}",
            self.observe, self.suggest, self.auto, self.cooldown
        )
    }
}

const BAR_CELLS: usize = 10;
const FULL_BLOCK: char = '\u{2588}';
const EMPTY_BLOCK: char = '\u{2591}';

/// Renders a value in `[0, 1]` as a ten-cell █/░ bar.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn score_bar(value: f64) -> String {
    let clamped = value.clamp(0.0, 1.0);
    let filled = ((clamped * BAR_CELLS as f64).round() as usize).min(BAR_CELLS);
    let mut bar = String::with_capacity(BAR_CELLS);
    for index in 0..BAR_CELLS {
        bar.push(if index < filled {
            FULL_BLOCK
        } else {
            EMPTY_BLOCK
        });
    }
    bar
}

/// Signed breakdown contribution rendered as `+██` / `-█` / blank bar.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn signed_bar(value: f64) -> String {
    if value.abs() < f64::EPSILON {
        return format!(" {EMPTY_BLOCK}");
    }
    let filled = ((value.abs().min(1.0) * BAR_CELLS as f64).round() as usize).clamp(1, BAR_CELLS);
    let sign = if value > 0.0 { '+' } else { '-' };
    let mut bar = String::new();
    bar.push(sign);
    for _ in 0..filled {
        bar.push(FULL_BLOCK);
    }
    bar
}

/// Accessible Vietnamese text table for one chart (spec §15.4A/§15.4D).
#[must_use]
pub fn text_alternative(snapshot: &ChartSnapshot) -> String {
    let identity = &snapshot.identity;
    let method = match identity.input_method {
        InputMethod::Telex => "Telex",
        InputMethod::Vni => "VNI",
    };
    let mut lines = Vec::new();
    lines.push(format!(
        "Biểu đồ học tập: {} → {} · {}",
        identity.original_nfc, identity.candidate_nfc, method
    ));
    if let Some(checkpoint) = snapshot.compaction_marker_at_ms {
        lines.push(format!("— Tổng hợp dữ liệu cũ (tại {checkpoint} ms) —"));
    }
    for point in &snapshot.points {
        let marker = point.marker.map_or(" ", ChartMarker::symbol);
        lines.push(format!(
            "t={} {} tin cậy {:.0}% · {}",
            point.at_ms,
            marker,
            point.confidence * 100.0,
            point.state_band.vietnamese_label()
        ));
    }
    if snapshot.points.is_empty() {
        lines.push("Chưa có sự kiện nào được ghi nhận.".to_owned());
    }
    lines.push(String::new());
    lines.push("Phân rã điểm hiện tại:".to_owned());
    let breakdown = &snapshot.breakdown;
    lines.push(format!(
        "Điểm nền generator             {}",
        score_bar(breakdown.generator_base)
    ));
    lines.push(format!(
        "Ảnh hưởng correction cá nhân   {}",
        signed_bar(breakdown.exact_correction)
    ));
    lines.push(format!(
        "Ảnh hưởng từ đứng trước        {}",
        signed_bar(breakdown.bigram)
    ));
    lines.push(format!(
        "Phạt do recent revert          {}",
        signed_bar(-breakdown.recent_revert_penalty)
    ));
    lines.push(format!(
        "Khoảng cách top1-top2          {:.2}",
        breakdown.top1_top2_margin
    ));
    lines.push(format!(
        "Kết luận                       {}",
        snapshot.conclusion
    ));
    lines.join("\r\n")
}

/// Band background colors matching spec §15.4A (gray/yellow/green + hatch).
#[must_use]
pub const fn band_color(band: ChartStateBand) -> COLORREF {
    match band {
        // COLORREF layout is 0x00BBGGRR.
        ChartStateBand::Observe => COLORREF(0x00EF_EFEA),
        ChartStateBand::Suggest => COLORREF(0x00CC_F2FF),
        ChartStateBand::Auto => COLORREF(0x00D5_D6EA),
        ChartStateBand::Cooldown => COLORREF(0x00E6_E6E6),
    }
}

/// Paints the timeline into an owner-draw rectangle.
///
/// Bands span equal slices per event, a confidence polyline overlays them, and
/// markers render as glyphs. Without a snapshot the area shows a hint line.
/// Plain GDI on the caller's DC — no web runtime.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn draw_timeline(dc: HDC, rect: &RECT, snapshot: Option<&ChartSnapshot>) {
    unsafe {
        let background = CreateSolidBrush(COLORREF(0x00FF_FFFF));
        FillRect(dc, rect, background);
        let _ = DeleteObject(background.into());
    }
    let Some(snapshot) = snapshot else {
        draw_hint(dc, rect, "Chọn một rule để xem biểu đồ học tập.");
        return;
    };
    let points = &snapshot.points;
    if points.is_empty() {
        draw_hint(dc, rect, "Chưa có sự kiện nào được ghi nhận.");
        return;
    }
    let width = (rect.right - rect.left).max(1);
    let slice = width / points.len() as i32;
    for (index, point) in points.iter().enumerate() {
        let left = rect.left + (index as i32) * slice;
        let right = if index + 1 == points.len() {
            rect.right
        } else {
            left + slice
        };
        let band_rect = RECT {
            left,
            top: rect.top,
            right,
            bottom: rect.bottom,
        };
        unsafe {
            let brush = CreateSolidBrush(band_color(point.state_band));
            FillRect(dc, &raw const band_rect, brush);
            let _ = DeleteObject(brush.into());
            if point.state_band == ChartStateBand::Cooldown {
                draw_hatch(dc, &band_rect);
            }
        }
    }
    draw_confidence_polyline(dc, rect, snapshot);
    draw_markers(dc, rect, snapshot);
}

fn draw_hint(dc: HDC, rect: &RECT, hint: &str) {
    let font = chart_font();
    let old_font = unsafe { SelectObject(dc, font.into()) };
    let mut text: Vec<u16> = hint.encode_utf16().collect();
    unsafe {
        let _ = SetTextColor(dc, COLORREF(0x00A0_A0A0));
        let _ = SetBkMode(dc, TRANSPARENT);
        let mut area = *rect;
        let _ = DrawTextW(
            dc,
            &mut text,
            &raw mut area,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let _ = SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
    }
}

fn draw_hatch(dc: HDC, rect: &RECT) {
    unsafe {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(0x00C0_B8B0));
        let old_pen = SelectObject(dc, pen.into());
        let height = rect.bottom - rect.top;
        let mut x = rect.left - height;
        while x < rect.right {
            let _ = MoveToEx(dc, x, rect.bottom, None);
            let _ = LineTo(dc, x + height, rect.top);
            x += 6;
        }
        let _ = SelectObject(dc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn draw_confidence_polyline(dc: HDC, rect: &RECT, snapshot: &ChartSnapshot) {
    let points = &snapshot.points;
    if points.len() < 2 {
        return;
    }
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    unsafe {
        let pen = CreatePen(PS_SOLID, 2, COLORREF(0x0060_3070));
        let old_pen = SelectObject(dc, pen.into());
        for (index, point) in points.iter().enumerate() {
            let x = rect.left + (width * index as i32) / (points.len() - 1) as i32;
            let y = rect.bottom - (f64::from(height) * point.confidence.clamp(0.0, 1.0)) as i32;
            if index == 0 {
                let _ = MoveToEx(dc, x, y, None);
            } else {
                let _ = LineTo(dc, x, y);
            }
        }
        let _ = SelectObject(dc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn draw_markers(dc: HDC, rect: &RECT, snapshot: &ChartSnapshot) {
    let points = &snapshot.points;
    let width = (rect.right - rect.left).max(1);
    let step = width / points.len() as i32;
    let font = chart_font();
    let old_font = unsafe { SelectObject(dc, font.into()) };
    unsafe {
        let _ = SetTextColor(dc, COLORREF(0x0020_2010));
        let _ = SetBkMode(dc, TRANSPARENT);
    }
    for (index, point) in points.iter().enumerate() {
        let Some(marker) = point.marker else {
            continue;
        };
        let center = rect.left + index as i32 * step + step / 2;
        let mut symbol_rect = RECT {
            left: center - 8,
            top: rect.top + 2,
            right: center + 8,
            bottom: rect.top + 18,
        };
        let mut text: Vec<u16> = marker.symbol().encode_utf16().collect();
        unsafe {
            let _ = DrawTextW(
                dc,
                &mut text,
                &raw mut symbol_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }
    unsafe {
        let _ = SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
    }
}

fn chart_font() -> HFONT {
    unsafe {
        CreateFontW(
            -12,
            0,
            0,
            0,
            FW_NORMAL.0.cast_signed(),
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            u32::from(DEFAULT_PITCH.0 | FF_DONTCARE.0),
            w!("Segoe UI"),
        )
    }
}

/// Owner-draw entry routed from `WM_DRAWITEM` for the chart canvas.
pub fn handle_draw_item(item: &DRAWITEMSTRUCT, snapshot: Option<&ChartSnapshot>) -> bool {
    draw_timeline(item.hDC, &item.rcItem, snapshot);
    true
}
