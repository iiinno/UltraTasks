use crate::{
    cli::Options,
    diagnostics::{Diagnostics, micros},
    settings::Settings,
};
use futures::channel::oneshot;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread::JoinHandle,
    time::Instant as Clock,
};
use ultratasks_core::*;

#[derive(Clone)]
pub enum Work {
    Open,
    Tasks(TaskQuery, TaskPageRequest),
    Rows(TaskQuery, Vec<TaskId>),
    Agenda(AgendaQuery, PageRequest<AgendaCursor>),
    Collections(PageRequest<CollectionCursor>),
    Detail(TaskId),
    Get(EntityId),
    SaveTask(Save<TaskId, TaskData>),
    SaveEvent(Save<EventId, EventData>),
    SaveBlock(Expected<TaskId>, Save<TimeBlockId, BlockData>),
    SaveCollection(CollectionSave),
    Delete(DeleteTarget),
    Undo(UndoTarget),
    Redo(RedoTarget),
    Settings(Settings),
}
pub struct Opened {
    pub info: StoreInfo,
    pub history: HistoryState,
    pub settings: Settings,
    pub warning: Option<String>,
}
pub enum Payload {
    Opened(Opened),
    Tasks(TaskPage),
    Rows(TaskRefresh),
    Agenda(Page<AgendaItem, AgendaCursor>),
    Collections(Page<Collection, CollectionCursor>),
    Detail(TaskDetail),
    Entity(Entity),
    Saved(Commit<()>),
    Settings(Settings),
}
#[derive(Debug)]
pub enum Failure {
    Core(Error),
    Local(String),
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(e) => f.write_str(&crate::presentation::feedback::user_error(e)),
            Self::Local(e) => f.write_str(e),
        }
    }
}
pub struct Response {
    pub request_id: u64,
    pub finished: Clock,
    pub result: std::result::Result<Payload, Failure>,
}
struct Envelope {
    id: u64,
    queued: Clock,
    work: Work,
    reply: oneshot::Sender<Response>,
}
#[derive(Clone)]
pub struct Client {
    sender: SyncSender<Option<Envelope>>,
    sequence: Arc<AtomicU64>,
}
pub struct Worker {
    client: Client,
    thread: JoinHandle<()>,
}
impl Client {
    pub fn request(
        &self,
        work: Work,
    ) -> std::result::Result<oneshot::Receiver<Response>, &'static str> {
        let (reply, receiver) = oneshot::channel();
        let envelope = Envelope {
            id: self.sequence.fetch_add(1, Ordering::Relaxed),
            queued: Clock::now(),
            work,
            reply,
        };
        match self.sender.try_send(Some(envelope)) {
            Ok(()) => Ok(receiver),
            Err(TrySendError::Full(_)) => {
                Err("処理待ちがいっぱいです。入力を保持しています。少し待って再試行してください")
            }
            Err(TrySendError::Disconnected(_)) => {
                Err("保存担当が終了しました。入力を保持しています。アプリを再起動してください")
            }
        }
    }
}
impl Worker {
    pub fn start(options: Options, diagnostics: Diagnostics) -> std::io::Result<Self> {
        let (sender, receiver) = sync_channel::<Option<Envelope>>(64);
        let client = Client {
            sender,
            sequence: Arc::new(AtomicU64::new(1)),
        };
        let thread = std::thread::Builder::new()
            .name("ultratasks-store".into())
            .spawn(move || {
                let mut store = None;
                let mut fixture = options.demo;
                while let Ok(Some(envelope)) = receiver.recv() {
                    let start = Clock::now();
                    diagnostics.sample(
                        "queue_wait",
                        envelope.id,
                        micros(start.duration_since(envelope.queued)),
                        1,
                    );
                    let result = if matches!(envelope.work, Work::Open) {
                        store.take();
                        (|| {
                            if let Some(config) = fixture.take() {
                                fixtures::seed_empty(&options.db, config)?;
                            }
                            let mut opened = Store::open(&options.db)?;
                            let info = opened.info().clone();
                            let history = opened.history()?;
                            let (settings, warning) = match Settings::load(&options.db) {
                                Ok(s) => (s, None),
                                Err(e) => (Settings::default(), Some(e)),
                            };
                            store = Some(opened);
                            Ok(Payload::Opened(Opened {
                                info,
                                history,
                                settings,
                                warning,
                            }))
                        })()
                    } else if let Work::Settings(settings) = envelope.work {
                        settings
                            .save(&options.db)
                            .map(|()| Payload::Settings(settings))
                            .map_err(Failure::Local)
                    } else if let Some(store) = store.as_mut() {
                        execute(store, envelope.work).map_err(Failure::Core)
                    } else {
                        Err(Failure::Local("保存先を開けていません".into()))
                    };
                    let finished = Clock::now();
                    diagnostics.sample(
                        "core_response",
                        envelope.id,
                        micros(finished.duration_since(start)),
                        u64::from(result.is_ok()),
                    );
                    let _ = envelope.reply.send(Response {
                        request_id: envelope.id,
                        finished,
                        result,
                    });
                }
            })?;
        Ok(Self { client, thread })
    }
    pub fn client(&self) -> Client {
        self.client.clone()
    }
    pub fn shutdown_after_event_loop(self) {
        let _ = self.client.sender.send(None);
        let _ = self.thread.join();
    }
}
fn erased<T>(commit: Commit<T>) -> Payload {
    Payload::Saved(Commit {
        value: (),
        operation: commit.operation,
        changed: commit.changed,
        generation: commit.generation,
        history: commit.history,
    })
}
fn execute(store: &mut Store, work: Work) -> Result<Payload> {
    Ok(match work {
        Work::Tasks(query, page) => Payload::Tasks(store.tasks(query, page)?),
        Work::Rows(query, ids) => Payload::Rows(store.task_rows(query, &ids)?),
        Work::Agenda(query, page) => Payload::Agenda(store.agenda(query, page)?),
        Work::Collections(page) => Payload::Collections(store.collections(page)?),
        Work::Detail(id) => Payload::Detail(store.task_detail(id)?),
        Work::Get(id) => Payload::Entity(store.get(id)?),
        Work::SaveTask(edit) => erased(store.save_task(edit)?),
        Work::SaveEvent(edit) => erased(store.save_event(edit)?),
        Work::SaveBlock(task, edit) => erased(store.save_time_block(task, edit)?),
        Work::SaveCollection(edit) => erased(store.save_collection(edit)?),
        Work::Delete(target) => erased(store.delete(target)?),
        Work::Undo(target) => erased(store.undo(target)?),
        Work::Redo(target) => erased(store.redo(target)?),
        Work::Open | Work::Settings(_) => return Err(Error::RecoveryRequired),
    })
}
