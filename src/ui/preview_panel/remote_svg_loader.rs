//! Universal image loader for the markdown preview: claims local `file://`
//! and remote `http(s)://` image URIs (svg/png/jpg/gif/webp), bypassing
//! egui_extras' built-ins entirely so we get full logging, system-proxy
//! support, a hard download timeout, and query-string-tolerant SVG matching.
//!
//! Ported/extended from MadEdit's RemoteSvgLoader.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use egui::load::{ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};

const FETCH_TIMEOUT_SECS: u64 = 15;

type Rasterized = Result<Arc<egui::ColorImage>, String>;

enum FetchState {
    InFlight(Receiver<Rasterized>),
    Done(Rasterized),
}

pub struct PreviewImageLoader {
    options: Arc<usvg_045::Options<'static>>,
    cache: Mutex<HashMap<(String, SizeHint), FetchState>>,
    /// Decoded animated gifs keyed by base URI (frames + frame durations).
    animated: Mutex<HashMap<String, GifState>>,
}

struct GifFrames {
    frames: Vec<Arc<egui::ColorImage>>,
    durations: Vec<Duration>,
}

enum GifState {
    InFlight(Receiver<Result<Arc<GifFrames>, String>>),
    Ready(Arc<GifFrames>),
}

impl Default for PreviewImageLoader {
    fn default() -> Self {
        Self {
            options: Arc::new(raster_options()),
            cache: Mutex::new(HashMap::new()),
            animated: Mutex::new(HashMap::new()),
        }
    }
}

fn image_extensions() -> &'static [&'static str] {
    // gif INCLUDED: built-in FileLoader hangs on `file:///G:/...` URIs
    // (parses as a UNC network path, os error 53, never Ready), so local
    // gifs must be read by our loader. Animation is reimplemented below
    // (frames decoded once, per-frame `#N` URIs + FrameDurations in ctx).
    &["svg", "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"]
}

/// URI path (before `?`/`#`) ends with a known image extension.
fn has_image_extension(uri: &str) -> bool {
    let base = uri.split('#').next().unwrap_or(uri);
    let path = base.split('?').next().unwrap_or(base);
    let lower = path.to_ascii_lowercase();
    image_extensions().iter().any(|ext| lower.ends_with(ext))
}

/// Claim file:// and http(s):// URIs with image extensions.
fn should_claim(uri: &str) -> bool {
    let base = uri.split('#').next().unwrap_or(uri);
    if base.starts_with("file://") {
        return has_image_extension(base);
    }
    if base.starts_with("http://") || base.starts_with("https://") {
        return has_image_extension(base);
    }
    false
}

fn raster_options() -> usvg_045::Options<'static> {
    let mut options = usvg_045::Options::default();
    let mut db = fontdb_sys::Database::new();
    db.load_system_fonts();
    options.fontdb = Arc::new(db);
    options
}

fn timeout() -> Duration {
    Duration::from_secs(FETCH_TIMEOUT_SECS)
}

/// Decode image bytes into a ColorImage. SVG goes through resvg (with system
/// fonts so badge text renders); raster formats through the `image` crate.
///
/// 坑 4/7 (MadEdit): SVGs may declare enormous native sizes (100k+ px) that
/// freeze resvg for minutes and panic past the texture limit. Parse the
/// natural size first and render with a `Scale` hint compressed so the
/// longest side stays within `max_texture_side`; egui scales down for
/// display, which looks lossless at panel sizes.
///
/// `#h=NN` URI fragment (from converted `<img height=NN>`) wins over native
/// size: the image is rendered/resized to that pixel height, aspect kept.
fn decode_image(
    uri: &str,
    bytes: &[u8],
    options: &usvg_045::Options<'static>,
    max_texture_side: usize,
) -> Rasterized {
    let hint_height = uri
        .split('#')
        .nth(1)
        .and_then(|frag| frag.strip_prefix("h="))
        .and_then(|h| h.parse::<u32>().ok())
        .filter(|h| *h > 0);

    let lower = uri.to_ascii_lowercase();
    // Strip #fragment and ?query BEFORE extension checks — a `#h=200` tail
    // otherwise breaks `.svg` detection and sends SVGs to the raster decoder.
    let base = lower.split('#').next().unwrap_or(&lower);
    let path = base.split('?').next().unwrap_or(base);
    let image = if path.ends_with(".svg") {
        let tree = usvg_045::Tree::from_data(bytes, options)
            .map_err(|e| format!("svg parse failed: {e}"))?;
        let natural = tree.size();
        let base_w = natural.width().max(1.0);
        let base_h = natural.height().max(1.0);

        // 坑 7: an explicit Height hint renders in milliseconds — use it
        // directly; otherwise clamp native size to the texture limit.
        let hint = match hint_height {
            Some(h) => SizeHint::Height(h),
            None => {
                let longest = base_w.max(base_h);
                let scale = if longest > max_texture_side as f32 {
                    max_texture_side as f32 / longest
                } else {
                    1.0
                };
                SizeHint::Scale(scale.into())
            }
        };
        let mut image = egui_extras::image::load_svg_bytes_with_size(bytes, hint, options)?;
        // load_svg_bytes_with_size stamps `source_size` with the SVG's NATIVE
        // size (e.g. 4000x3000); egui's `fit_to_original_size` then displays
        // the small rendered bitmap upscaled to native -> huge AND blurry.
        // Override with the actual rendered size so display == raster (1:1,
        // crisp, and exactly the height the README's `height` attr asked for).
        image.source_size = egui::Vec2::new(
            image.size[0] as f32,
            image.size[1] as f32,
        );
        clamp_texture_size(image, max_texture_side)
    } else {
        let decoded = image::load_from_memory(bytes).map_err(|e| format!("decode failed: {e}"))?;
        let mut rgba = decoded.to_rgba8();
        // Height hint for raster images: proportional resize after decode.
        if let Some(h) = hint_height {
            let nat_h = rgba.height().max(1) as f32;
            let scale = h as f32 / nat_h;
            let new_w = ((rgba.width() as f32) * scale).round().max(1.0) as u32;
            let resized = image::DynamicImage::ImageRgba8(rgba)
                .resize_exact(new_w, h, image::imageops::FilterType::Triangle);
            rgba = resized.to_rgba8();
        }
        egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            &rgba,
        )
    };
    Ok(Arc::new(clamp_texture_size(image, max_texture_side)))
}

/// egui asserts texture sides ≤ max_texture_side (2048 on glow); oversized
/// images PANIC, so every raster passes through this clamp (坑 5).
fn clamp_texture_size(mut image: egui::ColorImage, max_side: usize) -> egui::ColorImage {
    let [w, h] = image.size;
    let max = w.max(h);
    if max <= max_side {
        return image;
    }
    let scale = max_side as f32 / max as f32;
    let new_w = ((w as f32) * scale).round().max(1.0) as u32;
    let new_h = ((h as f32) * scale).round().max(1.0) as u32;
    // ColorImage.pixels is Vec<Color32>; flatten to RGBA bytes for `image`.
    let mut rgba_bytes = Vec::with_capacity(image.pixels.len() * 4);
    for c in &image.pixels {
        rgba_bytes.extend_from_slice(&[c.r(), c.g(), c.b(), c.a()]);
    }
    let rgba =
        image::RgbaImage::from_raw(w as u32, h as u32, rgba_bytes)
            .expect("pixel buffer size matches dimensions");
    let resized = image::DynamicImage::ImageRgba8(rgba)
        .resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
    egui::ColorImage::from_rgba_unmultiplied(
        [new_w as usize, new_h as usize],
        &resized.to_rgba8(),
    )
}

fn fetch_and_decode(
    uri: &str,
    options: Arc<usvg_045::Options<'static>>,
    timeout: Duration,
    max_texture_side: usize,
) -> Rasterized {
    let bytes = if let Some(path) = uri.strip_prefix("file://") {
        // Drop any #fragment (height hint) before touching the filesystem.
        let clean = path
            .split('#')
            .next()
            .unwrap_or(path)
            .trim_start_matches('/');
        std::fs::read(clean).map_err(|e| format!("read failed: {e}"))?
    } else {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        let mut response = agent
            .get(uri)
            .call()
            .map_err(|e| format!("download failed: {e}"))?;
        let status = response.status().as_u16();
        if status >= 400 {
            return Err(format!("download failed: HTTP {status}"));
        }
        let bytes = {
            use std::io::Read as _;
            let mut reader = response.body_mut().as_reader();
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .map_err(|e| format!("read body failed: {e}"))?;
            buf
        };
        bytes
    };
    decode_image(uri, &bytes, &options, max_texture_side)
}

impl PreviewImageLoader {
    pub const ID: &'static str = egui::generate_loader_id!(PreviewImageLoader);

    /// Install once per context (repeat calls are guarded by the caller).
    pub fn install(ctx: &egui::Context) {
        if !ctx.is_loader_installed(Self::ID) {
            ctx.add_image_loader(Arc::new(Self::default()));
        }
    }

    /// Load an animated gif: fetch/decode once in the background, then serve
    /// `frame_index` per request. Registers FrameDurations in ctx data so
    /// egui::Image cycles frames on its own.
    fn load_gif(
        &self,
        ctx: &egui::Context,
        base: &str,
        frame_index: usize,
    ) -> ImageLoadResult {
        let mut animated = self.animated.lock().unwrap();
        let mut needs_start = false;
        let mut load_error: Option<String> = None;
        let frames: Option<Arc<GifFrames>> = match animated.get(base) {
            Some(GifState::Ready(f)) => Some(Arc::clone(f)),
            Some(GifState::InFlight(rx)) => match rx.try_recv() {
                Ok(Ok(f)) => {
                    animated.insert(base.to_owned(), GifState::Ready(Arc::clone(&f)));
                    Some(f)
                }
                Ok(Err(e)) => {
                    animated.remove(base);
                    load_error = Some(e);
                    None
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    return Ok(ImagePoll::Pending { size: None });
                }
                Err(TryRecvError::Disconnected) => {
                    animated.remove(base);
                    needs_start = true;
                    None
                }
            },
            None => {
                needs_start = true;
                None
            }
        };

        if let Some(e) = load_error {
            return Err(LoadError::Loading(e));
        }

        if needs_start {
            log::info!("[PreviewImg] fetching animated gif {base}");
            let (tx, rx) = channel();
            animated.insert(base.to_owned(), GifState::InFlight(rx));
            drop(animated);
            let base_owned = base.to_owned();
            std::thread::Builder::new()
                .name("preview-gif-fetch".into())
                .spawn(move || {
                    let _ = tx.send(decode_gif(&base_owned, timeout()));
                })
                .ok();
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return Ok(ImagePoll::Pending { size: None });
        }

        match frames {
            Some(f) => {
                // Give egui::Image the frame durations so it cycles on its own.
                ctx.data_mut(|data| {
                    *data
                        .get_temp_mut_or_default(egui::Id::new(base.to_owned())) =
                        egui::widgets::FrameDurations::new(f.durations.clone())
                });
                let idx = frame_index.min(f.frames.len().saturating_sub(1));
                let image = f.frames[idx].clone();
                Ok(ImagePoll::Ready { image })
            }
            None => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                Ok(ImagePoll::Pending { size: None })
            }
        }
    }
}

/// Read + decode all frames of a gif file (local or remote).
fn decode_gif(uri: &str, timeout: Duration) -> Result<Arc<GifFrames>, String> {
    use image::AnimationDecoder as _;
    let bytes = if let Some(path) = uri.strip_prefix("file://") {
        let clean = path
            .split('#')
            .next()
            .unwrap_or(path)
            .trim_start_matches('/');
        std::fs::read(clean).map_err(|e| format!("read failed: {e}"))?
    } else {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        let mut response = agent
            .get(uri)
            .call()
            .map_err(|e| format!("download failed: {e}"))?;
        if response.status().as_u16() >= 400 {
            return Err(format!(
                "download failed: HTTP {}",
                response.status().as_u16()
            ));
        }
        let bytes = {
            use std::io::Read as _;
            let mut reader = response.body_mut().as_reader();
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .map_err(|e| format!("read body failed: {e}"))?;
            buf
        };
        bytes
    };

    let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&bytes))
        .map_err(|e| format!("gif decode: {e}"))?;
    let mut frames = Vec::new();
    let mut durations = Vec::new();
    for frame in decoder.into_frames() {
        let frame = frame.map_err(|e| format!("gif frame: {e}"))?;
        let delay: Duration = frame.delay().into();
        durations.push(delay);
        frames.push(Arc::new(egui::ColorImage::from_rgba_unmultiplied(
            [frame.buffer().width() as usize, frame.buffer().height() as usize],
            frame.buffer().as_flat_samples().as_slice(),
        )));
    }
    if frames.is_empty() {
        return Err("gif has no frames".into());
    }
    Ok(Arc::new(GifFrames { frames, durations }))
}

impl ImageLoader for PreviewImageLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str, size_hint: SizeHint) -> ImageLoadResult {
        // Animated gif branch: egui::Image cycles by requesting `uri#frame`
        // (egui::decode_animated_image_uri), so serve per-frame images here.
        if let Ok((anim_base, frame_index)) = egui::decode_animated_image_uri(uri) {
            if anim_base.ends_with(".gif") {
                return self.load_gif(ctx, anim_base, frame_index);
            }
        }
        if !should_claim(uri) {
            return Err(LoadError::NotSupported);
        }
        let key = (uri.to_owned(), size_hint);
        log::info!("[PreviewImg] load called uri={uri} hint={size_hint:?}");

        let mut cache = self.cache.lock().unwrap();
        let mut needs_start = false;

        if let Some(state) = cache.get_mut(&key) {
            match state {
                FetchState::Done(r) => {
                    let r = r.clone();
                    drop(cache);
                    return done_to_load_result(r);
                }
                FetchState::InFlight(rx) => match rx.try_recv() {
                    Ok(r) => {
                        *state = FetchState::Done(r.clone());
                        drop(cache);
                        log::info!("[PreviewImg] loaded {uri}");
                        return done_to_load_result(r);
                    }
                    Err(TryRecvError::Empty) => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(50));
                        return Ok(ImagePoll::Pending { size: None });
                    }
                    Err(TryRecvError::Disconnected) => needs_start = true,
                },
            }
        } else {
            needs_start = true;
        }

        if needs_start {
            log::info!("[PreviewImg] fetching {uri}");
            let options = Arc::clone(&self.options);
            let max_texture_side = ctx.input(|i| i.max_texture_side as usize);
            let (tx, rx) = channel();
            let uri_owned = key.0.clone();
            cache.insert(key, FetchState::InFlight(rx));
            drop(cache);
            std::thread::Builder::new()
                .name("preview-img-fetch".into())
                .spawn(move || {
                    let _ = tx.send(fetch_and_decode(
                        &uri_owned,
                        options,
                        timeout(),
                        max_texture_side,
                    ));
                })
                .ok();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        Ok(ImagePoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().unwrap().retain(|(u, _), _| u != uri);
    }

    fn forget_all(&self) {
        self.cache.lock().unwrap().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .unwrap()
            .values()
            .filter_map(|s| match s {
                FetchState::Done(Ok(img)) => Some(img.pixels.len() * 4),
                _ => None,
            })
            .sum()
    }
}

fn done_to_load_result(r: Rasterized) -> ImageLoadResult {
    r.map(|image| ImagePoll::Ready { image })
        .map_err(LoadError::Loading)
}

#[cfg(test)]
mod giant_svg_tests {
    use super::*;

    /// A 4000x3000 SVG must rasterize within the texture limit, keeping aspect
    /// ratio (MadEdit 坑4/5).
    #[test]
    fn giant_svg_is_clamped_to_texture_limit() {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="4000" height="3000"><rect width="4000" height="3000" fill="blue"/></svg>"#
        );
        let options = raster_options();
        let tree = usvg_045::Tree::from_data(svg.as_bytes(), &options).unwrap();
        let natural = tree.size();
        assert_eq!((natural.width(), natural.height()), (4000.0, 3000.0));
        let max_side = 2048usize;
        let image = egui_extras::image::load_svg_bytes_with_size(
            svg.as_bytes(),
            SizeHint::Scale((max_side as f32 / 4000.0).into()),
            &options,
        )
        .unwrap();
        let [w, h] = image.size;
        assert!(w <= max_side && h <= max_side, "clamped: {w}x{h}");
        let ratio = w as f32 / h as f32;
        assert!((ratio - 4.0 / 3.0).abs() < 0.05, "aspect kept: {ratio}");
    }

    /// A huge SVG truncated mid-file must not be silently misdecoded.
    #[test]
    fn truncated_svg_errors_instead_of_panic() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect"#;
        let options = raster_options();
        assert!(usvg_045::Tree::from_data(svg.as_bytes(), &options).is_err());
    }
}

#[cfg(test)]
mod height_branch_probe {
    use super::*;

    /// Reproduce the Height-hint path with the actual failing file.
    #[test]
    fn probe_height_branch_with_real_file() {
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\data\records\final_best_trousers.svg");
        let Ok(bytes) = std::fs::read(path) else { return };
        let options = raster_options();
        let result = decode_image(
            "file:///G:/x/final_best_trousers.svg#h=200",
            &bytes,
            &options,
            2048,
        );
        match result {
            Ok(img) => println!("OK size {:?}", img.size),
            Err(e) => println!("ERR: {e}"),
        }
    }
}

#[cfg(test)]
mod gif_probe {
    use eframe::egui::load::{ImagePoll, SizeHint};

    #[test]
    fn probe_builtin_gif_loader_with_real_demo_gif() {
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\data\demo.gif");
        let Ok(bytes) = std::fs::read(path) else { return };
        let ctx = eframe::egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let uri = "file:///G:/back/share/sparrow/里程碑备份/sparrow/data/demo.gif";
        for _ in 0..10 {
            match ctx.try_load_image(uri, SizeHint::default()) {
                Ok(ImagePoll::Ready { image }) => {
                    println!("READY size {:?}", image.size);
                    return;
                }
                Ok(ImagePoll::Pending { .. }) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(e) => {
                    println!("ERR {e:?}");
                    return;
                }
            }
        }
        println!("STILL PENDING after 1s");
    }
}

#[cfg(test)]
mod gif_in_app_probe {
    use eframe::egui::load::{ImagePoll, SizeHint};

    /// Mirror the app exactly: install built-ins, then our loader last,
    /// poll the gif URI like egui::Image does.
    #[test]
    fn probe_gif_through_full_loader_chain() {
        let path = std::path::Path::new(r"G:\back\share\sparrow\里程碑备份\sparrow\data\demo.gif");
        let Ok(bytes) = std::fs::read(path) else { return };
        let ctx = eframe::egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        super::PreviewImageLoader::install(&ctx);
        let uri = "file:///G:/back/share/sparrow/里程碑备份/sparrow/data/demo.gif";
        ctx.include_bytes(uri.to_string(), bytes);
        for frame in 0..30 {
            match ctx.try_load_image(uri, SizeHint::default()) {
                Ok(ImagePoll::Ready { image }) => {
                    println!("READY frame{frame} size {:?}", image.size);
                    return;
                }
                Ok(ImagePoll::Pending { .. }) => {
                    println!("frame{frame}: pending");
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    println!("ERR frame{frame}: {e:?}");
                    return;
                }
            }
        }
        println!("NEVER READY");
    }
}
