// High-performance window capture via Windows.Graphics.Capture.
//
// Why WGC: it is GPU-accelerated, works for a specific window regardless of
// focus or occlusion, and needs no window activation — which is exactly what
// agent observation needs (the user keeps working in other apps while the
// agent screenshots the game).

use anyhow::{Context, Result};
use std::sync::mpsc;
use std::time::Duration;
use windows_capture::{
    capture::{CaptureControl, Context as CaptureContext, GraphicsCaptureApiHandler},
    frame::{Frame, FrameBuffer},
    graphics_capture_api::InternalCaptureControl,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};

use super::window;

/// A captured frame encoded as PNG.
#[derive(Debug, Clone)]
pub struct CapturedImage {
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

/// Handler that grabs the first arriving frame, encodes it, and stops the
/// capture session.
struct OneShotCapture {
    /// Sends the encoded frame (or an error string) back to the caller.
    /// `None` after the result has been delivered.
    sender: Option<mpsc::Sender<Result<CapturedImage, String>>>,
}

impl GraphicsCaptureApiHandler for OneShotCapture {
    type Flags = mpsc::Sender<Result<CapturedImage, String>>;
    type Error = String;

    fn new(ctx: CaptureContext<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            sender: Some(ctx.flags),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let Some(sender) = self.sender.take() else {
            capture_control.stop();
            return Ok(());
        };

        let result = encode_frame(frame)
            .map_err(|e| format!("Failed to encode captured frame: {}", e));

        let _ = sender.send(result);
        capture_control.stop();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Err(
                "Game window closed before a frame could be captured".into(),
            ));
        }
        Ok(())
    }
}

/// Extract RGBA pixels from a frame buffer, removing row padding.
fn extract_rgba(buffer: &mut FrameBuffer) -> Result<(Vec<u8>, u32, u32)> {
    let width = buffer.width();
    let height = buffer.height();
    let row_pitch = buffer.row_pitch() as usize;
    let row_bytes = width as usize * 4;

    let raw = buffer.as_raw_buffer();
    let mut pixels = Vec::with_capacity(row_bytes * height as usize);

    if row_pitch == row_bytes {
        pixels.extend_from_slice(&raw[..row_bytes * height as usize]);
    } else {
        for y in 0..height as usize {
            let start = y * row_pitch;
            let end = start + row_bytes;
            if end > raw.len() {
                anyhow::bail!(
                    "Frame buffer too small: need {} bytes, have {}",
                    end,
                    raw.len()
                );
            }
            pixels.extend_from_slice(&raw[start..end]);
        }
    }
    Ok((pixels, width, height))
}

/// Encode the frame as PNG using the `image` crate (RGBA8).
fn encode_frame(frame: &mut Frame) -> Result<CapturedImage> {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};

    let mut buffer = frame.buffer().context("Failed to access frame buffer")?;
    let (pixels, width, height) = extract_rgba(&mut buffer)?;

    let mut png_bytes: Vec<u8> = Vec::new();
    let encoder = PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
        .context("PNG encoding failed")?;

    Ok(CapturedImage {
        width,
        height,
        png_bytes,
    })
}

/// Capture a single PNG frame from a resolved game window. Blocks up to
/// `timeout` waiting for the first frame. The capture runs on its own
/// thread and does not steal focus from any window.
pub fn capture_window_png(resolved: window::ResolvedWindow, timeout_secs: u64) -> Result<CapturedImage> {
    let (sender, receiver) = mpsc::channel();

    let settings = Settings::new(
        resolved.window,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        sender.clone(),
    );

    // Start the capture session without blocking this thread; we wait on the
    // channel with our own timeout instead.
    let capture: CaptureControl<_, String> = OneShotCapture::start_free_threaded(settings)
        .map_err(|e| anyhow::anyhow!("Failed to start window capture: {}", e))?;

    let result = receiver.recv_timeout(Duration::from_secs(timeout_secs.max(1)));
    // Stop the capture session. Failure here is benign (the channel has
    // already delivered or timed out), so the Result is intentionally dropped.
    let _ = capture.stop();

    match result {
        Ok(Ok(image)) => Ok(image),
        Ok(Err(e)) => anyhow::bail!("{}", e),
        Err(_) => anyhow::bail!(
            "Timed out waiting for a frame from the game window after {}s",
            timeout_secs
        ),
    }
}

/// Capture a screenshot with channel fallback:
/// 1. Despotes in-game framebuffer capture (works even when minimized,
///    returns the pure client area).
/// 2. Windows.Graphics.Capture window capture (fallback when Despotes is
///    not present/running).
/// Returns the image plus which source produced it.
pub fn capture_instance_best(
    instance_dir: &std::path::Path,
    instance_name: &str,
    pid: Option<u32>,
    timeout_secs: u64,
) -> Result<(CapturedImage, &'static str)> {
    if let Ok(bytes) = futures_lite_block(super::client::capture_image(instance_dir)) {
        if let Some((w, h)) = png_dimensions(&bytes) {
            return Ok((
                CapturedImage { width: w, height: h, png_bytes: bytes },
                "despotes",
            ));
        }
    }
    let img = capture_instance_png(instance_name, pid, timeout_secs)?;
    Ok((img, "wgc"))
}

/// Decode PNG header dimensions without full rasterization.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    use image::Limits;
    use image::ImageDecoder;
    let mut opts = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    opts.limits(Limits::no_limits());
    let reader = opts.into_decoder().ok()?;
    let (w, h) = reader.dimensions();
    Some((w, h))
}

fn futures_lite_block<F: std::future::Future<Output = T>, T>(fut: F) -> T {
    // Block on a tokio runtime handle when available; otherwise spin up a
    // temporary runtime. This helper is called from blocking CLI paths.
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        tokio::runtime::Runtime::new().unwrap().block_on(fut)
    }
}

/// Locate the instance's game window and capture one PNG frame.
pub fn capture_instance_png(
    instance_name: &str,
    pid: Option<u32>,
    timeout_secs: u64,
) -> Result<CapturedImage> {
    let resolved = window::find_for_instance(instance_name, pid)?;
    capture_window_png(resolved, timeout_secs)
}

#[cfg(test)]
mod tests {
    // Frame capture requires a live window; the padding-strip and PNG
    // encoding paths are covered by manual integration testing (see
    // tests/game_control_manual.md in Alpha 6 docs).

    #[test]
    fn test_padding_math() {
        // Verify the index arithmetic used by the padded-row path.
        let width: usize = 5;
        let height: usize = 3;
        let row_pitch = 24; // > width * 4 = 20
        let row_bytes = width * 4;
        let raw = vec![0u8; row_pitch * height];
        let mut pixels = Vec::new();
        for y in 0..height {
            let start = y * row_pitch;
            let end = start + row_bytes;
            pixels.extend_from_slice(&raw[start..end]);
        }
        assert_eq!(pixels.len(), row_bytes * height);
    }
}
