//! Composites a Letterboxd poster wall in-process: layout math
//! (`poster_layout`), fetching, resizing, and edge-fill compositing, all
//! backed by the `image` crate.
//!
//! Poster tiles are fetched over the network, one request per tile — sequential
//! fetching was measured (against real Letterboxd data, a 40-tile 4K wall) at
//! ~72s total with individual requests ranging 60ms-9s, which reads as a hang
//! with no feedback. Fetching is parallelized across a small worker pool
//! (`POSTER_FETCH_WORKERS`) and reports progress via `on_progress` so callers
//! can drive a real progress bar instead of a fake animation.

use crate::poster_layout::{self, PosterLayout};
use crate::wallerror::WallError;
use image::{ImageBuffer, Rgb, RgbImage, imageops::FilterType};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

/// Same ceiling the remote API applied as a CPU/memory guard — kept here so
/// a very large `poster_count` can't make local generation unreasonably slow.
const MAX_TILES: u32 = 60;
const JPEG_QUALITY: u8 = 90;

/// Posters are small static CDN images with no cold-start concern (unlike the
/// diary RSS fetch) — bound each one well above the ~9s worst case observed
/// in testing so one bad connection can't tie up a worker for the full 60s
/// the RSS/general request timeout allows.
const POSTER_FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Fetches are network-bound (mostly idle waiting on the CDN), so a modest
/// worker count buys most of the available speedup without hammering the CDN
/// with a burst of 60 simultaneous connections.
const POSTER_FETCH_WORKERS: usize = 8;

fn fetch_and_resize_poster(
    poster_url: &str,
    target_w: u32,
    target_h: u32,
    client: &reqwest::blocking::Client,
) -> Option<RgbImage> {
    let sized_url = poster_layout::sized_poster_url(poster_url, target_h);
    let bytes = client
        .get(&sized_url)
        .timeout(POSTER_FETCH_TIMEOUT)
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    Some(image::imageops::resize(
        &decoded.to_rgb8(),
        target_w,
        target_h,
        FilterType::Lanczos3,
    ))
}

/// Fetches+resizes every core-grid tile concurrently across a small worker
/// pool, returning results indexed the same way the grid is (`entries[idx]`).
/// Calls `on_progress(done, total)` after each tile completes (from whichever
/// worker thread finished it — callers relying on this for UI updates must
/// tolerate out-of-order/concurrent calls).
fn fetch_all_posters(
    entries: &[Option<String>],
    core_count: u32,
    poster_w: u32,
    poster_h: u32,
    client: &reqwest::blocking::Client,
    on_progress: &(dyn Fn(u32, u32) + Sync),
) -> Vec<Option<RgbImage>> {
    let total = core_count as usize;
    let queue: Mutex<VecDeque<usize>> = Mutex::new((0..total).collect());
    let done = AtomicU32::new(0);
    let (tx, rx) = mpsc::channel::<(usize, Option<RgbImage>)>();

    std::thread::scope(|scope| {
        let workers = POSTER_FETCH_WORKERS.min(total).max(1);
        for _ in 0..workers {
            let tx = tx.clone();
            let queue = &queue;
            let done = &done;
            scope.spawn(move || {
                loop {
                    let idx = { queue.lock().unwrap().pop_front() };
                    let Some(idx) = idx else { break };
                    let img = entries
                        .get(idx)
                        .and_then(|o| o.as_deref())
                        .and_then(|url| fetch_and_resize_poster(url, poster_w, poster_h, client));
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    on_progress(n, total as u32);
                    let _ = tx.send((idx, img));
                }
            });
        }
        drop(tx);
    });

    let mut results: Vec<Option<RgbImage>> = (0..total).map(|_| None).collect();
    for (idx, img) in rx.try_iter() {
        results[idx] = img;
    }
    results
}

/// Composites the whole wall and returns encoded JPEG bytes, ready to write
/// straight to disk. `on_progress(done, total)` fires as poster tiles finish
/// fetching (see `fetch_all_posters`); pass `&|_, _| {}` to ignore.
pub fn generate(
    username: &str,
    w: u32,
    h: u32,
    poster_count: Option<u32>,
    client: &reqwest::blocking::Client,
    on_progress: &(dyn Fn(u32, u32) + Sync),
) -> Result<Vec<u8>, WallError> {
    let entries = crate::letterboxd::fetch_diary_posters(username, client)?;
    if entries.is_empty() {
        return Err(WallError::Other(format!(
            "No diary entries found for '{username}'"
        )));
    }

    let available = poster_count
        .map(|c| c.min(entries.len() as u32))
        .unwrap_or(entries.len() as u32);
    let target = available.min(MAX_TILES);

    let rows = poster_layout::rows_for_count(w, h, target)
        .ok_or_else(|| WallError::Other("invalid wallpaper size".to_string()))?;
    let layout = poster_layout::compute_edge_fill_layout(w, h, rows);
    let poster_w = layout.poster_w.round() as u32;
    let poster_h = layout.poster_h.round() as u32;

    let mut posters = fetch_all_posters(
        &entries,
        layout.core_count,
        poster_w,
        poster_h,
        client,
        on_progress,
    );

    // A poster tile failing individually is tolerated (composite_core_grid
    // just leaves that grid cell white) - but if every single tile failed,
    // that's not a partial degradation, it's total network failure, and the
    // "wall" would just be a blank white image. Applying that as the desktop
    // wallpaper/lock screen would look like data loss with no error shown,
    // so treat it as a failure instead - retryable, since it's almost
    // certainly the same transient condition `fetch_wallpaper`'s retry loop
    // already handles for the RSS fetch.
    let succeeded = posters.iter().filter(|p| p.is_some()).count();
    if layout.core_count > 0 && succeeded == 0 {
        return Err(WallError::Network(format!(
            "all {} poster downloads failed for '{username}'",
            layout.core_count
        )));
    }

    let mut canvas: RgbImage = ImageBuffer::from_pixel(w, h, Rgb([255, 255, 255]));
    let strip_w = layout.strip_w.round() as u32;
    // Row -> that row's edge-column poster (already resized), kept alive for
    // edge-fill reuse below instead of re-fetching the same poster twice.
    let mut edge_col_images: HashMap<u32, RgbImage> = HashMap::new();

    composite_core_grid(
        &mut canvas,
        &mut posters,
        &layout,
        poster_h,
        strip_w,
        &mut edge_col_images,
    );
    fill_edge_strips(&mut canvas, &edge_col_images, poster_h, strip_w, w);

    // Belt-and-suspenders for the original "blank wallpaper" report: whatever
    // the cause (all tiles failed, a layout regression placing posters
    // off-canvas, ...), never hand a nearly all-white image to Windows. The
    // existing cache and desktop are left untouched instead.
    if is_blank(&canvas) {
        return Err(WallError::Network(format!(
            "generated wallpaper would be blank ({succeeded}/{} posters downloaded) for \
            '{username}'",
            layout.core_count
        )));
    }

    crate::applog::info(&format!(
        "composited {succeeded}/{} posters for '{username}' at {w}x{h}",
        layout.core_count
    ));

    encode_jpeg(&canvas)
}

/// True when at least 95% of the canvas is still the pure-white background,
/// i.e. the composited result would read as a blank wallpaper. Posters never
/// cover a meaningful area with exact `#FFFFFF`, so this can't misfire on a
/// legitimately bright wall.
fn is_blank(canvas: &RgbImage) -> bool {
    let total = canvas.width() as u64 * canvas.height() as u64;
    if total == 0 {
        return true;
    }
    let white = canvas.pixels().filter(|p| p.0 == [255, 255, 255]).count() as u64;
    white * 100 >= total * 95
}

/// Draws each already-fetched poster tile onto the canvas at its grid
/// position, single-threaded (fast — pure CPU compositing, no network).
fn composite_core_grid(
    canvas: &mut RgbImage,
    posters: &mut [Option<RgbImage>],
    layout: &PosterLayout,
    poster_h: u32,
    strip_w: u32,
    edge_col_images: &mut HashMap<u32, RgbImage>,
) {
    for row in 0..layout.rows {
        for col in 0..layout.max_cols {
            let idx = (row * layout.max_cols + col) as usize;
            if idx as u32 >= layout.core_count {
                break;
            }
            let Some(resized) = posters.get_mut(idx).and_then(|o| o.take()) else {
                continue;
            };

            let x = (layout.core_offset_x + col as f64 * layout.poster_w).round() as i64;
            let y = (row * poster_h) as i64;
            image::imageops::overlay(canvas, &resized, x, y);

            if strip_w > 0 && col == layout.edge_col {
                edge_col_images.insert(row, resized);
            }
        }
    }
}

/// Duplicates each row's edge-column poster on both sides, clipped to strip
/// width, to fill the leftover margin the core grid doesn't cover.
fn fill_edge_strips(
    canvas: &mut RgbImage,
    edge_col_images: &HashMap<u32, RgbImage>,
    poster_h: u32,
    strip_w: u32,
    w: u32,
) {
    for (&row, resized) in edge_col_images {
        let y = (row * poster_h) as i64;
        let poster_w = resized.width();

        let left = image::imageops::crop_imm(resized, 0, 0, strip_w, poster_h).to_image();
        image::imageops::overlay(canvas, &left, 0, y);

        let right =
            image::imageops::crop_imm(resized, poster_w - strip_w, 0, strip_w, poster_h).to_image();
        image::imageops::overlay(canvas, &right, (w - strip_w) as i64, y);
    }
}

fn encode_jpeg(canvas: &RgbImage) -> Result<Vec<u8>, WallError> {
    let mut jpeg_bytes = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder
        .encode_image(canvas)
        .map_err(|e| WallError::Other(format!("JPEG encode failed: {e}")))?;
    Ok(jpeg_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_detection_rejects_white_and_accepts_a_real_wall() {
        let white: RgbImage = ImageBuffer::from_pixel(100, 100, Rgb([255, 255, 255]));
        assert!(
            is_blank(&white),
            "an untouched canvas must be treated as blank"
        );

        let mut mostly_white = white.clone();
        for y in 0..10 {
            for x in 0..10 {
                mostly_white.put_pixel(x, y, Rgb([10, 20, 30]));
            }
        }
        assert!(
            is_blank(&mostly_white),
            "1% non-white is still visually a blank wallpaper"
        );

        let mut poster_wall = white;
        for y in 0..100 {
            for x in 0..100 {
                if x < 20 {
                    poster_wall.put_pixel(x, y, Rgb([30 + x as u8, 60, 90]));
                }
            }
        }
        assert!(
            !is_blank(&poster_wall),
            "20% coverage is a real poster wall"
        );
    }
}
