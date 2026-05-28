use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use bytes::{Bytes, BytesMut};
use termcolor::{Buffer, Color, ColorSpec, WriteColor as _};
use tokio::sync::{broadcast, mpsc};

pub const ERASE_PROGRESS_SPINNER: &str = "\r                        \r";
pub const ERASE_PROGRESS_SPINNER_BUF: &[u8] = ERASE_PROGRESS_SPINNER.as_bytes();

pub(super) fn ansi_paint(fg: Color, bg: Color, text: &str) -> String {
    let mut buf = Buffer::ansi();
    let _ = buf.set_color(ColorSpec::new().set_fg(Some(fg)).set_bg(Some(bg)));
    let _ = write!(buf, "{text}");
    let _ = buf.reset();
    String::from_utf8_lossy(buf.as_slice()).to_string()
}

#[derive(Clone)]
pub struct ServiceOutput {
    progress_visible: Arc<AtomicBool>,
    abort_tx: mpsc::Sender<()>,
    output_tx: broadcast::Sender<Bytes>,
}

impl ServiceOutput {
    pub fn new() -> Self {
        let progress_visible = Arc::new(AtomicBool::new(false));
        let (abort_tx, mut abort_rx) = mpsc::channel(1);
        let output_tx = broadcast::channel(32).0;

        tokio::spawn({
            let output_tx = output_tx.clone();
            let progress_visible = progress_visible.clone();
            let ticks = "⠁⠁⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠈⠈".chars().collect::<Vec<_>>();
            let mut tick_index = 0;
            async move {
                loop {
                    tokio::select! {
                        _ = abort_rx.recv() => {
                            return;
                        }
                        () = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                            if progress_visible.load(std::sync::atomic::Ordering::Relaxed) {
                                tick_index = (tick_index + 1) % ticks.len();
                                #[allow(clippy::indexing_slicing)]
                                let tick = ticks[tick_index];
                                let badge = ansi_paint(Color::Black, Color::Blue, &format!(" {tick} Warpgate connecting "));
                                let mut output = BytesMut::with_capacity(ERASE_PROGRESS_SPINNER_BUF.len() + badge.len());
                                output.extend_from_slice(ERASE_PROGRESS_SPINNER_BUF);
                                output.extend_from_slice(badge.as_bytes());
                                let _ = output_tx.send(output.freeze());
                            }
                        }
                    }
                }
            }
        });

        Self {
            progress_visible,
            abort_tx,
            output_tx,
        }
    }

    pub fn show_progress(&self) {
        self.progress_visible
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn stop_progress(&self) {
        self.progress_visible
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Bytes> {
        self.output_tx.subscribe()
    }
}

impl Drop for ServiceOutput {
    fn drop(&mut self) {
        let signal = std::mem::replace(&mut self.abort_tx, mpsc::channel(1).0);
        tokio::spawn(async move { signal.send(()).await });
    }
}
