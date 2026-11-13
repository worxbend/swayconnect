//! Rasterising untrusted icon files (`docs/DESIGN.md` 11.5). The daemon never decodes
//! them itself: [`ChildRasteriser`] runs `swayconnectd --raster`, which calls
//! [`child_main`] under memory, CPU and wall-time limits.

use std::io::{self, Read as _, Write as _};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use resvg::usvg;
use rustix::process::{Resource, Rlimit, setrlimit};
use swayconnect_protocol::limits::{MAX_ICON_PNG_BYTES, MAX_ICON_SIDE_PX};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use super::png;

/// Largest icon file read from disk.
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
/// Largest PNG side the child decodes.
pub const MAX_SOURCE_SIDE_PX: u32 = 1024;
pub const MAX_SVG_NODES: usize = 10_000;
/// The child is killed after this much wall time.
pub const CHILD_WALL_TIME: Duration = Duration::from_secs(3);
const CHILD_ADDRESS_SPACE_BYTES: u64 = 256 * 1024 * 1024;
const CHILD_CPU_SECONDS: u64 = 2;
/// The argument that makes `swayconnectd` run [`child_main`].
pub const RASTER_ARG: &str = "--raster";

/// Why an icon file gave no PNG.
#[derive(Debug, thiserror::Error)]
pub enum RasterError {
    #[error("source is larger than {MAX_SOURCE_BYTES} bytes")]
    SourceTooLarge,
    #[error("PNG declares {0} x {1} pixels")]
    SourceTooWide(u32, u32),
    #[error("PNG: {0}")]
    Png(String),
    #[error("SVG: {0}")]
    Svg(#[from] usvg::Error),
    #[error("SVG uses filters")]
    SvgFilters,
    #[error("SVG has more than {MAX_SVG_NODES} nodes")]
    SvgTooManyNodes,
    #[error("image has no area")]
    Empty,
    #[error("PNG output is not servable")]
    OutputNotServable,
    #[error("raster child: {0}")]
    Io(#[from] io::Error),
    #[error("raster child ran longer than {CHILD_WALL_TIME:?} and was killed")]
    TimedOut,
    #[error("raster child failed: {0}")]
    ChildFailed(ExitStatus),
}

/// Turns icon file bytes into a servable PNG. Called on the icon worker thread, one call
/// at a time.
pub trait Rasteriser: Send + 'static {
    fn rasterise(&self, source: &[u8]) -> Result<Vec<u8>, RasterError>;
}

/// Runs `<program> --raster` per icon: cleared environment, source on stdin, PNG on stdout.
#[derive(Debug, Clone)]
pub struct ChildRasteriser {
    program: PathBuf,
}

impl ChildRasteriser {
    /// `program` is the `swayconnectd` binary; `main.rs` passes its own path.
    pub fn new(program: PathBuf) -> ChildRasteriser {
        ChildRasteriser { program }
    }
}

impl Rasteriser for ChildRasteriser {
    fn rasterise(&self, source: &[u8]) -> Result<Vec<u8>, RasterError> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(RasterError::SourceTooLarge);
        }
        let mut child = Command::new(&self.program)
            .arg(RASTER_ARG)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (stdin, stdout) = (child.stdin.take(), child.stdout.take());
        let child = Arc::new(Mutex::new(child));
        let (finished, watchdog) = start_watchdog(Arc::clone(&child));
        let output = exchange(stdin, stdout, source);
        if output.is_err() {
            kill(&child);
        }
        drop(finished);
        let timed_out = watchdog.join().unwrap_or(true);
        let status = child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .wait()?;
        if timed_out {
            return Err(RasterError::TimedOut);
        }
        let output = output?;
        if !status.success() {
            return Err(RasterError::ChildFailed(status));
        }
        if !png::is_servable(&output) {
            return Err(RasterError::OutputNotServable);
        }
        Ok(output)
    }
}

/// Kills the child unless the returned sender is dropped within [`CHILD_WALL_TIME`].
/// The thread answers whether it killed. The child is reaped only after the thread ends,
/// so a kill never reaches a reused pid.
fn start_watchdog(child: Arc<Mutex<Child>>) -> (mpsc::Sender<()>, thread::JoinHandle<bool>) {
    let (finished, wait_for_finish) = mpsc::channel::<()>();
    let watchdog = thread::spawn(move || {
        let timed_out = matches!(
            wait_for_finish.recv_timeout(CHILD_WALL_TIME),
            Err(RecvTimeoutError::Timeout)
        );
        if timed_out {
            kill(&child);
        }
        timed_out
    });
    (finished, watchdog)
}

fn kill(child: &Mutex<Child>) {
    // An error means the child already exited, which is what the kill is for.
    let _ = child.lock().unwrap_or_else(PoisonError::into_inner).kill();
}

/// Writes the source, then reads at most one byte more than a servable PNG. Returns when
/// the child closes stdout: it does that only by exiting, or by being killed.
fn exchange(
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::process::ChildStdout>,
    source: &[u8],
) -> Result<Vec<u8>, RasterError> {
    let (Some(mut stdin), Some(stdout)) = (stdin, stdout) else {
        return Err(io::Error::other("child pipes missing").into());
    };
    stdin.write_all(source)?;
    drop(stdin);
    let mut output = Vec::new();
    stdout
        .take(MAX_ICON_PNG_BYTES as u64 + 1)
        .read_to_end(&mut output)?;
    if output.len() > MAX_ICON_PNG_BYTES {
        return Err(RasterError::OutputNotServable);
    }
    Ok(output)
}

/// The `swayconnectd --raster` process: limits itself, reads the source from stdin, writes
/// the PNG to stdout. Must run before any other thread starts, so `main.rs` calls it before
/// the async runtime exists.
pub fn child_main() -> ExitCode {
    if limit_this_process().is_err() {
        return ExitCode::FAILURE;
    }
    let mut source = Vec::new();
    let read = io::stdin()
        .lock()
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut source);
    if read.is_err() || source.len() > MAX_SOURCE_BYTES {
        return ExitCode::FAILURE;
    }
    let Ok(output) = render(&source) else {
        return ExitCode::FAILURE;
    };
    let mut stdout = io::stdout().lock();
    match stdout.write_all(&output).and_then(|()| stdout.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// `RLIMIT_AS` 256 MiB and `RLIMIT_CPU` 2 s; no core file when the CPU limit hits.
fn limit_this_process() -> io::Result<()> {
    let limit = |value: u64| Rlimit {
        current: Some(value),
        maximum: Some(value),
    };
    setrlimit(Resource::Core, limit(0))?;
    setrlimit(Resource::As, limit(CHILD_ADDRESS_SPACE_BYTES))?;
    setrlimit(Resource::Cpu, limit(CHILD_CPU_SECONDS))?;
    Ok(())
}

/// A servable PNG from PNG or SVG bytes. The checks of `docs/DESIGN.md` 11.5 run before
/// anything is rendered.
pub fn render(source: &[u8]) -> Result<Vec<u8>, RasterError> {
    let pixmap = if png::has_signature(source) {
        render_png(source)?
    } else {
        render_svg(source)?
    };
    let output = pixmap
        .encode_png()
        .map_err(|error| RasterError::Png(error.to_string()))?;
    if png::is_servable(&output) {
        Ok(output)
    } else {
        Err(RasterError::OutputNotServable)
    }
}

fn render_png(source: &[u8]) -> Result<Pixmap, RasterError> {
    let (width, height) =
        png::declared_size(source).ok_or_else(|| RasterError::Png("no IHDR".to_owned()))?;
    if width > MAX_SOURCE_SIDE_PX || height > MAX_SOURCE_SIDE_PX {
        return Err(RasterError::SourceTooWide(width, height));
    }
    let decoded =
        Pixmap::decode_png(source).map_err(|error| RasterError::Png(error.to_string()))?;
    // A bitmap is never enlarged: that adds bytes and no detail.
    let scale = fit_scale(decoded.width() as f32, decoded.height() as f32)?.min(1.0);
    let mut target = target_pixmap(decoded.width() as f32, decoded.height() as f32, scale)?;
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let transform = Transform::from_scale(scale, scale);
    target.draw_pixmap(0, 0, decoded.as_ref(), &paint, transform, None);
    Ok(target)
}

fn render_svg(source: &[u8]) -> Result<Pixmap, RasterError> {
    let options = usvg::Options {
        // No file and no embedded image is loaded: an icon is one self-contained file.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(source, &options)?;
    if !tree.filters().is_empty() {
        return Err(RasterError::SvgFilters);
    }
    if node_count_exceeds(tree.root(), MAX_SVG_NODES) {
        return Err(RasterError::SvgTooManyNodes);
    }
    let size = tree.size();
    let scale = fit_scale(size.width(), size.height())?;
    let mut target = target_pixmap(size.width(), size.height(), scale)?;
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut target.as_mut(),
    );
    Ok(target)
}

/// Counts groups, paths, images and texts, including the subtrees of patterns, clip paths,
/// masks and nested images, and stops past `limit`. Recursion depth is bounded by the
/// parser's nesting limit.
fn node_count_exceeds(root: &usvg::Group, limit: usize) -> bool {
    fn count(group: &usvg::Group, seen: &mut usize, limit: usize) -> bool {
        for node in group.children() {
            *seen += 1;
            if *seen > limit {
                return true;
            }
            let mut exceeded = false;
            node.subroots(|subroot| exceeded = exceeded || count(subroot, seen, limit));
            let in_child = matches!(node, usvg::Node::Group(child) if count(child, seen, limit));
            if exceeded || in_child {
                return true;
            }
        }
        false
    }
    count(root, &mut 0, limit)
}

/// The factor that brings the longer side to 128 px.
fn fit_scale(width: f32, height: f32) -> Result<f32, RasterError> {
    let longer = width.max(height);
    if !(longer.is_finite() && longer > 0.0) {
        return Err(RasterError::Empty);
    }
    Ok(MAX_ICON_SIDE_PX as f32 / longer)
}

fn target_pixmap(width: f32, height: f32, scale: f32) -> Result<Pixmap, RasterError> {
    let side = |length: f32| ((length * scale).round() as u32).clamp(1, MAX_ICON_SIDE_PX);
    Pixmap::new(side(width), side(height)).ok_or(RasterError::Empty)
}

#[cfg(test)]
#[path = "raster_tests.rs"]
mod tests;
