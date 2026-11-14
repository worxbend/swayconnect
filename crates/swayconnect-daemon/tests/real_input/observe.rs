//! What `real_input` observes outside the daemon: the FIFO a target window's `cat` writes,
//! screenshots of the headless output, and whether fuzzel holds the keyboard.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rustix::fs::{CWD, Mode, mkfifoat};
use serde_json::Value;
use swayconnect_testkit::RealSession;

/// foot, the pty and `cat` need milliseconds; this only bounds a broken run.
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(20);
/// fuzzel maps within milliseconds; this only bounds a broken run.
const GRAB_TIMEOUT: Duration = Duration::from_secs(20);
const GRAB_POLL: Duration = Duration::from_millis(50);

/// What a target window's `cat` wrote into its FIFO, read on a thread. `cat` gets a line
/// at each Return (the pty is in canonical mode); its end, after Ctrl+C, is the FIFO's EOF.
pub struct Typed {
    chunks: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
}

impl Typed {
    pub fn open(fifo: &Path) -> Typed {
        mkfifoat(CWD, fifo, Mode::from_raw_mode(0o600)).expect("mkfifo");
        let (sender, chunks) = mpsc::channel();
        let path = fifo.to_owned();
        std::thread::spawn(move || {
            // Blocks until cat opens the FIFO; an empty chunk marks the end.
            let mut file = File::open(path).expect("open the FIFO");
            let mut buffer = [0; 4096];
            loop {
                let read = file.read(&mut buffer).expect("read the FIFO");
                if sender.send(buffer[..read].to_vec()).is_err() || read == 0 {
                    return;
                }
            }
        });
        Typed {
            chunks,
            seen: Vec::new(),
        }
    }

    /// Waits until everything `cat` wrote so far is `expected`.
    pub fn wait_for(&mut self, expected: &str) {
        let deadline = Instant::now() + OUTPUT_TIMEOUT;
        while self.seen != expected.as_bytes() {
            assert!(
                expected.as_bytes().starts_with(&self.seen),
                "typed {:?}, expected {expected:?}",
                String::from_utf8_lossy(&self.seen)
            );
            let chunk = self.next_chunk(deadline, expected);
            assert!(!chunk.is_empty(), "cat ended before {expected:?}");
            self.seen.extend(chunk);
        }
    }

    /// Waits for `cat`'s end and returns all it wrote.
    pub fn end(mut self) -> String {
        let deadline = Instant::now() + OUTPUT_TIMEOUT;
        loop {
            let chunk = self.next_chunk(deadline, "the end");
            if chunk.is_empty() {
                return String::from_utf8(self.seen).expect("UTF-8");
            }
            self.seen.extend(chunk);
        }
    }

    fn next_chunk(&self, deadline: Instant, waiting_for: &str) -> Vec<u8> {
        let left = deadline.saturating_duration_since(Instant::now());
        self.chunks.recv_timeout(left).unwrap_or_else(|_| {
            panic!(
                "no more output while waiting for {waiting_for:?}; typed {:?}",
                String::from_utf8_lossy(&self.seen)
            )
        })
    }
}

pub fn shot_path(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("real-session");
    fs::create_dir_all(&dir).expect("create the screenshot dir");
    dir.join(format!("r3-{name}.png"))
}

/// Waits until fuzzel holds the keyboard: sway then marks no node as focused (a layer
/// surface is not in the tree), the same signal the daemon's guard reads. fuzzel draws no
/// sway event, so this polls the tree.
pub fn wait_for_launcher_grab(session: &RealSession) {
    let deadline = Instant::now() + GRAB_TIMEOUT;
    while Instant::now() < deadline {
        let tree: Value =
            serde_json::from_str(&session.swaymsg(&["-t", "get_tree"])).expect("sway tree JSON");
        if !has_focused_node(&tree) {
            return;
        }
        std::thread::sleep(GRAB_POLL);
    }
    panic!("fuzzel did not take the keyboard within {GRAB_TIMEOUT:?}");
}

fn has_focused_node(node: &Value) -> bool {
    node["focused"] == true
        || ["nodes", "floating_nodes"].iter().any(|key| {
            node[*key]
                .as_array()
                .is_some_and(|children| children.iter().any(has_focused_node))
        })
}
