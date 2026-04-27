use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use indicatif::ProgressBar;

use super::render::ProgressRenderState;

const MESSAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub(super) struct MessageRefresh {
    stop_sender: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    enabled: bool,
}

impl MessageRefresh {
    pub(super) const fn enabled() -> Self {
        Self {
            stop_sender: None,
            thread: None,
            enabled: true,
        }
    }

    pub(super) fn disable(&mut self) {
        self.enabled = false;
    }

    pub(super) fn start(
        &mut self,
        progress_bar: ProgressBar,
        render_state: Arc<Mutex<ProgressRenderState>>,
    ) {
        if !self.enabled || self.thread.is_some() {
            return;
        }

        let (stop_sender, stop_receiver) = mpsc::channel();
        self.stop_sender = Some(stop_sender);

        self.thread = Some(thread::spawn(move || {
            loop {
                match stop_receiver.recv_timeout(MESSAGE_REFRESH_INTERVAL) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        let Ok(render_state) = render_state.lock() else {
                            break;
                        };
                        let message = render_state.progress_message();

                        drop(render_state);

                        progress_bar.set_message(message);
                        progress_bar.tick();
                    }
                }
            }
        }));
    }

    pub(super) fn stop(&mut self) {
        if let Some(stop_sender) = self.stop_sender.take() {
            let _ = stop_sender.send(());
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for MessageRefresh {
    fn drop(&mut self) {
        self.stop();
    }
}
