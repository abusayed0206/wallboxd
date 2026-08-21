//! Shared layout math for a natural-size edge-fill poster collage.
//! Individual posters keep their true aspect ratio; edge strips are filled
//! by duplicating the middle column (clipped to strip width).

use std::sync::LazyLock;

pub const POSTER_RATIO: f64 = 230.0 / 345.0; // ~= 0.6667

/// Letterboxd's poster CDN only honors this fixed set of crop heights at an
/// exact 2:3 ratio — confirmed by direct testing against the CDN.
const SAFE_POSTER_HEIGHTS: [u32; 8] = [345, 450, 690, 750, 900, 1500, 1800, 3000];

static CROP_SIZE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"-0-\d+-0-\d+-crop").unwrap());

/// Rewrites a Letterboxd poster URL's embedded crop size to the smallest
/// CDN-safe height that's still >= `target_h`.
pub fn sized_poster_url(url: &str, target_h: u32) -> String {
    let h = SAFE_POSTER_HEIGHTS
        .iter()
        .copied()
        .find(|&candidate| candidate >= target_h)
        .unwrap_or(*SAFE_POSTER_HEIGHTS.last().unwrap());
    let w = (h / 3) * 2;
    CROP_SIZE_RE
        .replace(url, format!("-0-{w}-0-{h}-crop"))
        .into_owned()
}

pub struct PosterLayout {
    pub rows: u32,
    pub max_cols: u32,
    pub core_count: u32,
    pub poster_h: f64,
    pub poster_w: f64,
    pub core_offset_x: f64,
    pub strip_w: f64,
    pub edge_col: u32,
}

fn max_cols_for(width: u32, poster_w: f64) -> u32 {
    ((width as f64 / poster_w).floor() as u32).max(1)
}

/// Every valid edge-fill (rows, `max_cols`, count) option, ascending by
/// count. Rows capped at 12; stops once count exceeds `available_count`.
fn edge_fill_options(width: u32, height: u32, available_count: u32) -> Vec<(u32, u32, u32)> {
    let mut options = Vec::new();
    for rows in 1..=12u32 {
        let poster_h = height as f64 / rows as f64;
        let poster_w = poster_h * POSTER_RATIO;
        let max_cols = max_cols_for(width, poster_w);
        let count = rows * max_cols;
        if count > available_count {
            break;
        }
        options.push((rows, max_cols, count));
    }
    options
}

/// Full edge-fill geometry for a given row count.
pub fn compute_edge_fill_layout(width: u32, height: u32, rows: u32) -> PosterLayout {
    let poster_h = height as f64 / rows as f64;
    let poster_w = poster_h * POSTER_RATIO;
    let max_cols = max_cols_for(width, poster_w);
    let core_count = rows * max_cols;
    let core_grid_w = max_cols as f64 * poster_w;
    let core_offset_x = (width as f64 - core_grid_w) / 2.0;
    let strip_w = core_offset_x;
    let edge_col = max_cols.saturating_sub(1) / 2;
    PosterLayout {
        rows,
        max_cols,
        core_count,
        poster_h,
        poster_w,
        core_offset_x,
        strip_w,
        edge_col,
    }
}

/// Rows for a given target poster count, or `None` if the size can't fit
/// even one poster. Falls back to the closest offered count <= requested.
pub fn rows_for_count(width: u32, height: u32, count: u32) -> Option<u32> {
    let options = edge_fill_options(width, height, count);
    if let Some((rows, _, _)) = options.iter().find(|&&(_, _, c)| c == count) {
        return Some(*rows);
    }
    let mut best_rows = None;
    for rows in 1..=12u32 {
        let poster_h = height as f64 / rows as f64;
        let poster_w = poster_h * POSTER_RATIO;
        let max_cols = max_cols_for(width, poster_w);
        let opt_count = rows * max_cols;
        if opt_count <= count {
            best_rows = Some(rows);
        }
    }
    best_rows
}
