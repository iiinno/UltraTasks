use serde::Serialize;
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread::JoinHandle,
    time::Instant,
};

#[derive(Clone)]
pub struct Diagnostics {
    sender: Option<SyncSender<Sample>>,
    origin: Instant,
    dropped: Arc<AtomicU64>,
}
#[derive(Serialize)]
struct Sample {
    kind: &'static str,
    request_id: u64,
    elapsed_us: u64,
    duration_us: u64,
    count: u64,
}
pub struct LogGuard {
    thread: Option<JoinHandle<()>>,
}
impl Diagnostics {
    pub fn start(directory: Option<&Path>) -> std::io::Result<(Self, LogGuard)> {
        let origin = Instant::now();
        let dropped = Arc::new(AtomicU64::new(0));
        let Some(directory) = directory else {
            return Ok((
                Self {
                    sender: None,
                    origin,
                    dropped,
                },
                LogGuard { thread: None },
            ));
        };
        std::fs::create_dir_all(directory)?;
        let path = directory.join(format!(
            "desktop-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let (sender, receiver) = sync_channel::<Sample>(4096);
        let dropped_log = dropped.clone();
        let thread = std::thread::Builder::new().name("ultratasks-diagnostics".into()).spawn(move || {
            let _ = writeln!(file, "{{\"kind\":\"metadata\",\"frame_metric\":\"render_callback_cpu_only\",\"presentation_time\":\"not_observed\"}}");
            while let Ok(sample) = receiver.recv() {
                if serde_json::to_writer(&mut file, &sample).is_err() || writeln!(file).is_err() { break; }
            }
            let _ = writeln!(file, "{{\"kind\":\"dropped_samples\",\"count\":{}}}", dropped_log.load(Ordering::Relaxed));
            let _ = file.flush();
        })?;
        Ok((
            Self {
                sender: Some(sender),
                origin,
                dropped,
            },
            LogGuard {
                thread: Some(thread),
            },
        ))
    }
    pub fn sample(&self, kind: &'static str, request_id: u64, duration_us: u64, count: u64) {
        if let Some(sender) = &self.sender
            && sender
                .try_send(Sample {
                    kind,
                    request_id,
                    elapsed_us: micros(self.origin.elapsed()),
                    duration_us,
                    count,
                })
                .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
impl LogGuard {
    pub fn join(self) {
        if let Some(thread) = self.thread {
            let _ = thread.join();
        }
    }
}
pub fn micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}
