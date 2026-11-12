use crate::{
    cli::Options,
    diagnostics::{Diagnostics, micros},
};
pub(crate) use crate::{
    presentation::{
        agenda as time,
        editor::{Draft, DueKind, Target},
        feedback::{InputArea, Validation, user_error},
        task_list::{PageMode, TaskCache},
    },
    settings::{MotionMode, Settings, ThemeMode},
    theme::{MotionSpec, Tokens},
    views::{button, focus_outline, input_frame},
    worker::{Client, Failure, Payload, Work},
};
pub(crate) use gpui_kit::{
    Entity,
    component::{
        Disableable, StyledExt, WindowExt,
        button::ButtonVariants,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
    },
    prelude::*,
    *,
};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant as Clock},
};
pub(crate) use ultratasks_core::{self as core, *};

actions!(
    ultratasks,
    [
        NewTask,
        FindTask,
        NewEvent,
        PreviousDay,
        NextDay,
        Today,
        EditSelected,
        SaveEditor,
        Cancel,
        UndoApp,
        RedoApp,
        PickDate,
        SelectNext,
        SelectPrevious,
        OpenSelected,
        CompleteSelected,
        PickerLeft,
        PickerRight,
        PickerUp,
        PickerDown,
        PickerPreviousMonth,
        PickerNextMonth,
        PickerApply
    ]
);
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewTask, Some("UltraTasks")),
        KeyBinding::new("ctrl-f", FindTask, Some("UltraTasks")),
        KeyBinding::new("ctrl-k", FindTask, Some("UltraTasks")),
        KeyBinding::new("ctrl-shift-e", NewEvent, Some("UltraTasks")),
        KeyBinding::new("alt-pageup", PreviousDay, Some("UltraTasks")),
        KeyBinding::new("alt-pagedown", NextDay, Some("UltraTasks")),
        KeyBinding::new("alt-home", Today, Some("UltraTasks")),
        KeyBinding::new("f2", EditSelected, Some("UltraTasks")),
        KeyBinding::new("ctrl-enter", SaveEditor, Some("UltraTasks")),
        KeyBinding::new("escape", Cancel, Some("UltraTasks")),
        KeyBinding::new("ctrl-z", UndoApp, Some("UltraTasks && !Input")),
        KeyBinding::new("ctrl-shift-z", RedoApp, Some("UltraTasks && !Input")),
        KeyBinding::new("ctrl-d", PickDate, Some("UltraTasks")),
        KeyBinding::new("down", SelectNext, Some("TaskList")),
        KeyBinding::new("up", SelectPrevious, Some("TaskList")),
        KeyBinding::new("enter", OpenSelected, Some("TaskList")),
        KeyBinding::new("space", CompleteSelected, Some("TaskList")),
        KeyBinding::new("left", PickerLeft, Some("DatePicker && !Input")),
        KeyBinding::new("right", PickerRight, Some("DatePicker && !Input")),
        KeyBinding::new("up", PickerUp, Some("DatePicker && !Input")),
        KeyBinding::new("down", PickerDown, Some("DatePicker && !Input")),
        KeyBinding::new("pageup", PickerPreviousMonth, Some("DatePicker")),
        KeyBinding::new("pagedown", PickerNextMonth, Some("DatePicker")),
        KeyBinding::new("enter", PickerApply, Some("DatePicker && !Input")),
    ]);
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Filters {
    pub list: Option<ListId>,
    pub status: StatusFilter,
    pub due: u8,
    pub blocks: BlockFilter,
}
impl Default for Filters {
    fn default() -> Self {
        Self {
            list: None,
            status: StatusFilter::Open,
            due: 0,
            blocks: BlockFilter::Any,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FormStamp {
    pub serial: u64,
    pub version: u64,
}
impl FormStamp {
    fn accepts(self, saved: Self) -> bool {
        self == saved
    }
}
pub(crate) struct Form {
    pub draft: Draft,
    pub title_input: Entity<TextareaState>,
    pub fields: [Entity<InputState>; 4],
    pub collection: Entity<gpui_kit::component::select::SelectState<Vec<CollectionOption>>>,
    pub dirty: bool,
    pub stamp: FormStamp,
    pub validation: Option<Validation>,
    pub conflict: Option<String>,
    pub _subscriptions: Vec<Subscription>,
    pub submit_ready: bool,
    pub drag_candidate: bool,
}
impl Form {
    pub(crate) fn changed(&mut self) {
        self.dirty = true;
        self.drag_candidate = false;
        self.stamp.version += 1;
        self.validation = None;
    }
}
#[derive(Clone)]
pub(crate) struct CollectionOption {
    id: core::EntityId,
    title: SharedString,
}
fn collection_options(collections: &[Collection], draft: &Draft) -> Vec<CollectionOption> {
    collections
        .iter()
        .filter_map(|collection| match collection {
            Collection::List(list) if matches!(draft.target, Target::Task { .. }) => {
                Some(CollectionOption {
                    id: core::EntityId::List(list.key.id),
                    title: list.data.as_str().to_owned().into(),
                })
            }
            Collection::Calendar(calendar)
                if matches!(draft.target, Target::Event { .. } | Target::Block { .. }) =>
            {
                Some(CollectionOption {
                    id: core::EntityId::Calendar(calendar.key.id),
                    title: calendar.data.as_str().to_owned().into(),
                })
            }
            _ => None,
        })
        .collect()
}
impl gpui_kit::component::select::SelectItem for CollectionOption {
    type Value = core::EntityId;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &Self::Value {
        &self.id
    }
}
#[derive(Clone)]
pub(crate) enum Navigation {
    Tasks,
    Calendar,
    List(Option<ListId>),
    QuickInput,
    SearchInput,
    Event,
    Day(Date),
    Task(TaskId),
    ListTask(TaskId, bool),
    Entity(core::EntityId),
    Collection(bool),
    Quit,
}
impl Navigation {
    fn is_available(&self, tasks: &TaskCache) -> bool {
        match self {
            Self::ListTask(id, _) => tasks.visible_selection(Some(*id)).is_some(),
            _ => true,
        }
    }
}
#[derive(Clone)]
pub(crate) enum Popup {
    QuickTask,
    Filters(Filters),
    Settings(Settings),
    Date,
    Collections,
    Leave(Navigation),
    Delete(DeleteTarget),
}
#[derive(Clone)]
pub(crate) enum WriteOrigin {
    Quick { title: String, version: u64 },
    Form { stamp: FormStamp },
    Direct,
}
pub(crate) struct PendingWrite {
    pub work: Work,
    pub origin: WriteOrigin,
}
impl WriteOrigin {
    fn matches_form(&self, current: Option<FormStamp>) -> bool {
        matches!(self, Self::Form { stamp } if current.is_some_and(|current| current.accepts(*stamp)))
    }
}
impl PendingWrite {
    fn can_retry(&self, current: Option<FormStamp>) -> bool {
        !matches!(self.origin, WriteOrigin::Form { .. }) || self.origin.matches_form(current)
    }
    fn abandon_form(failed: &mut Option<Self>, recovery: bool) -> bool {
        if !recovery
            && failed
                .as_ref()
                .is_some_and(|w| matches!(w.origin, WriteOrigin::Form { .. }))
        {
            *failed = None;
            true
        } else {
            false
        }
    }
}
pub(crate) struct Drag {
    pub source: DragSource,
    pub origin: Point<Pixels>,
    pub active: bool,
    pub minute: Option<i64>,
}
#[derive(Clone)]
pub(crate) enum DragSource {
    Task(TaskSummary),
    Block(TimeBlock, TaskSummary),
}
#[derive(Clone)]
enum ReadContext {
    Open,
    Tasks(u64, PageMode),
    Rows(u64),
    Agenda(u64, bool, bool),
    Collections,
    Detail(u64, bool),
    ListDetail(u64, TaskId, bool),
    ExistingBlock(u64, TimeBlockId),
    Entity(u64),
    Conflict(FormStamp),
    Recovery,
    Settings,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadScope {
    Open,
    Tasks,
    Rows,
    Agenda,
    AllDay,
    Collections,
    Selection,
    Conflict,
    Recovery,
    Settings,
}
impl ReadContext {
    fn selection_is_current(
        &self,
        epoch: u64,
        selected: Option<TaskId>,
        tasks: &TaskCache,
    ) -> bool {
        match self {
            Self::ListDetail(request, id, _) => {
                *request == epoch && tasks.visible_selection(selected) == Some(*id)
            }
            Self::Detail(request, _) | Self::ExistingBlock(request, _) | Self::Entity(request) => {
                *request == epoch
            }
            _ => true,
        }
    }
    fn scope(&self) -> ReadScope {
        match self {
            Self::Open => ReadScope::Open,
            Self::Tasks(..) => ReadScope::Tasks,
            Self::Rows(..) => ReadScope::Rows,
            Self::Agenda(_, true, _) => ReadScope::AllDay,
            Self::Agenda(_, false, _) => ReadScope::Agenda,
            Self::Collections => ReadScope::Collections,
            Self::Detail(..)
            | Self::ListDetail(..)
            | Self::ExistingBlock(..)
            | Self::Entity(..) => ReadScope::Selection,
            Self::Conflict(..) => ReadScope::Conflict,
            Self::Recovery => ReadScope::Recovery,
            Self::Settings => ReadScope::Settings,
        }
    }
}
#[derive(Default)]
struct FailedReads(Vec<(Work, ReadContext, String)>);
impl FailedReads {
    fn clear(&mut self, scope: ReadScope) {
        self.0.retain(|(_, context, _)| context.scope() != scope);
    }
    fn record(&mut self, work: Work, context: ReadContext, message: String) {
        self.clear(context.scope());
        self.0.push((work, context, message));
    }
    fn any(&self) -> bool {
        !self.0.is_empty()
    }
}
pub struct Desktop {
    pub(crate) client: Client,
    pub(crate) options: Options,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) info: Option<StoreInfo>,
    pub(crate) history: HistoryState,
    pub(crate) commit_generation: u64,
    pub(crate) settings: Settings,
    pub(crate) tokens: Tokens,
    pub(crate) motion: MotionSpec,
    pub(crate) focus: FocusHandle,
    pub(crate) list_focus: FocusHandle,
    pub(crate) popup_focus: FocusHandle,
    pub(crate) return_focus: Option<FocusHandle>,
    pub(crate) dialog_open: bool,
    pub(crate) popup_resume_quick: bool,
    pub(crate) calendar_view: bool,
    pub(crate) search_open: bool,
    pub(crate) quick: Entity<InputState>,
    pub(crate) search: Entity<InputState>,
    pub(crate) date_input: Entity<InputState>,
    pub(crate) date_ready: bool,
    pub(crate) date_validation: Option<String>,
    pub(crate) quick_validation: Option<String>,
    pub(crate) quick_version: u64,
    pub(crate) quick_id: Option<TaskId>,
    pub(crate) quick_list: Option<ListId>,
    pub(crate) quick_ready: bool,
    pub(crate) search_text: String,
    pub(crate) tasks: TaskCache,
    pub(crate) task_list: ListState,
    pub(crate) filters: Filters,
    pub(crate) selected: Option<TaskId>,
    pub(crate) saved_task: Option<TaskId>,
    pub(crate) form: Option<Form>,
    pub(crate) popup: Option<Popup>,
    pub(crate) status: String,
    pub(crate) error: bool,
    pub(crate) read_error: bool,
    failed_reads: FailedReads,
    pub(crate) recovery: bool,
    pub(crate) busy: bool,
    pub(crate) pending_write: Option<PendingWrite>,
    pub(crate) failed_write: Option<PendingWrite>,
    pub(crate) task_loading: bool,
    pub(crate) task_epoch: u64,
    pub(crate) task_pending: Option<(TaskPosition, PageMode)>,
    search_timer: Option<gpui_kit::Task<()>>,
    pub(crate) agenda: Vec<AgendaItem>,
    pub(crate) all_day: Vec<AgendaItem>,
    pub(crate) agenda_total: u64,
    pub(crate) all_day_total: u64,
    pub(crate) agenda_loaded: bool,
    pub(crate) all_day_loaded: bool,
    pub(crate) agenda_next: Option<AgendaCursor>,
    pub(crate) all_day_next: Option<AgendaCursor>,
    pub(crate) agenda_epoch: u64,
    pub(crate) all_day_epoch: u64,
    pub(crate) agenda_loading: bool,
    pub(crate) agenda_pending: bool,
    pub(crate) all_day_loading: bool,
    pub(crate) all_day_pending: bool,
    pub(crate) agenda_scroll: ScrollHandle,
    pub(crate) agenda_range: Option<TimedWindow>,
    pub(crate) agenda_candidates: bool,
    pub(crate) moved_from: std::collections::HashMap<core::EntityId, core::Instant>,
    pub(crate) collections: Vec<Collection>,
    pub(crate) collections_next: Option<CollectionCursor>,
    pub(crate) collections_previous: Option<CollectionCursor>,
    pub(crate) selection_epoch: u64,
    pub(crate) drag: Option<Drag>,
    pub(crate) suppress_click: bool,
    pub(crate) compact_agenda: bool,
    pub(crate) allow_close: bool,
    pub(crate) quit_after_save: bool,
    pub(crate) after_save: Option<Navigation>,
    pub(crate) frame: Option<Rc<Cell<bool>>>,
    pub(crate) rendered_rows: u64,
    first_data_recorded: bool,
    pub(crate) form_serial: u64,
    pub(crate) focus_editor_input: bool,
    pub(crate) subscriptions: Vec<Subscription>,
    pub(crate) now: Option<core::Instant>,
}
impl Desktop {
    pub fn new(
        options: Options,
        client: Client,
        diagnostics: Diagnostics,
        frame: Option<Rc<Cell<bool>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = Settings::default();
        let (tokens, motion) = settings.apply(cx);
        let quick = cx.new(|cx| InputState::new(window, cx).placeholder("次に取り組むこと"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("タスク名を検索"));
        let date_input = cx.new(|cx| InputState::new(window, cx).default_value(options.day.iso()));
        let focus = cx.focus_handle();
        let list_focus = cx.focus_handle();
        let task_list = ListState::new(0, ListAlignment::Top, px(168.));
        let weak = cx.weak_entity();
        task_list.set_scroll_handler(move |event, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                if this.task_loading {
                    return;
                }
                if event.visible_range.end + 20 >= this.tasks.rows.len() && this.tasks.later {
                    this.page_tasks(false, window, cx);
                } else if event.visible_range.start < 20 && this.tasks.earlier {
                    this.page_tasks(true, window, cx);
                }
            });
        });
        let mut this = Self {
            client,
            options,
            diagnostics,
            info: None,
            history: HistoryState {
                undo: None,
                redo: None,
            },
            commit_generation: 0,
            settings,
            tokens,
            motion,
            focus,
            list_focus,
            popup_focus: cx.focus_handle(),
            return_focus: None,
            dialog_open: false,
            popup_resume_quick: false,
            calendar_view: false,
            search_open: false,
            quick,
            search,
            date_input,
            date_ready: true,
            date_validation: None,
            quick_validation: None,
            quick_version: 0,
            quick_id: None,
            quick_list: None,
            quick_ready: true,
            search_text: String::new(),
            tasks: TaskCache::default(),
            task_list,
            filters: Filters::default(),
            selected: None,
            saved_task: None,
            form: None,
            popup: None,
            status: "保存先を開いています…".into(),
            error: false,
            read_error: false,
            failed_reads: FailedReads::default(),
            recovery: false,
            busy: false,
            pending_write: None,
            failed_write: None,
            task_loading: false,
            task_epoch: 0,
            task_pending: None,
            search_timer: None,
            agenda: Vec::new(),
            all_day: Vec::new(),
            agenda_total: 0,
            all_day_total: 0,
            agenda_loaded: false,
            all_day_loaded: false,
            agenda_next: None,
            all_day_next: None,
            agenda_epoch: 0,
            all_day_epoch: 0,
            agenda_loading: false,
            agenda_pending: false,
            all_day_loading: false,
            all_day_pending: false,
            agenda_scroll: ScrollHandle::new(),
            agenda_range: None,
            agenda_candidates: false,
            moved_from: std::collections::HashMap::new(),
            collections: Vec::new(),
            collections_next: None,
            collections_previous: None,
            selection_epoch: 0,
            drag: None,
            suppress_click: false,
            compact_agenda: false,
            allow_close: false,
            quit_after_save: false,
            after_save: None,
            frame,
            rendered_rows: 0,
            first_data_recorded: false,
            form_serial: 0,
            focus_editor_input: false,
            subscriptions: Vec::new(),
            now: None,
        };
        this.subscriptions.push(cx.subscribe_in(
            &this.date_input,
            window,
            |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.date_ready = false;
                    if this.date_validation.take().is_some() {
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. }
                    if this.date_ready && matches!(this.popup, Some(Popup::Date)) =>
                {
                    this.date_ready = false;
                    this.apply_picker(window, cx);
                }
                InputEvent::Blur => cx.notify(),
                _ => {}
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.quick,
            window,
            |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    let started = Clock::now();
                    this.quick_version += 1;
                    this.quick_ready = false;
                    if this.quick_validation.take().is_some() {
                        cx.notify();
                    }
                    this.diagnostics
                        .sample("input_handler_cpu", 0, micros(started.elapsed()), 1);
                }
                InputEvent::PressEnter { .. } if this.quick_ready => {
                    this.quick_ready = false;
                    this.add_task(window, cx);
                }
                _ => {}
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.search,
            window,
            |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = this.search.read(cx).value().to_string();
                    if this.search_text == text {
                        return;
                    }
                    this.search_text = text;
                    this.invalidate_task_scope();
                    let epoch = this.task_epoch;
                    this.search_timer = Some(cx.spawn_in(window, async move |weak, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let _ = weak.update_in(cx, |this, window, cx| {
                            if this.task_epoch == epoch {
                                this.query_tasks(
                                    TaskPosition::First,
                                    PageMode::Replace,
                                    window,
                                    cx,
                                );
                            }
                        });
                    }));
                    cx.notify();
                }
            },
        ));
        this.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() && !this.busy {
                    let had_drag = this.drag.take().is_some();
                    let candidate = this.form.as_ref().is_some_and(|form| form.drag_candidate);
                    if candidate {
                        this.discard_form();
                        this.clear_popup(window, cx);
                        this.restore_focus(window, cx);
                    }
                    if had_drag || candidate {
                        this.status = "仮配置を取り消しました".into();
                        cx.notify();
                    }
                }
                if window.is_window_active() {
                    let (t, motion) = this.settings.apply(cx);
                    this.tokens = t;
                    this.motion = motion;
                }
            }));
        this.subscriptions
            .push(cx.observe_window_appearance(window, |this, _, cx| {
                let (t, motion) = this.settings.apply(cx);
                this.tokens = t;
                this.motion = motion;
                cx.notify();
            }));
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                if this.allow_close {
                    return true;
                }
                if this.busy || this.dirty(cx) || this.error {
                    this.open_popup(Popup::Leave(Navigation::Quit), window, cx);
                    false
                } else {
                    true
                }
            })
            .unwrap_or(true)
        });
        this.focus.focus(window, cx);
        this.initial_scroll();
        this.read(Work::Open, ReadContext::Open, window, cx);
        this.schedule_clock(window, cx);
        this
    }
    pub(crate) fn dirty(&self, cx: &App) -> bool {
        self.form.as_ref().is_some_and(|f| f.dirty)
            || !self.quick.read(cx).value().trim().is_empty()
    }
    pub(crate) fn composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut fields = vec![
            self.quick.clone(),
            self.search.clone(),
            self.date_input.clone(),
        ];
        if let Some(form) = &self.form {
            if form.title_input.update(cx, |state, cx| {
                state.marked_text_range(window, cx).is_some()
            }) {
                return true;
            }
            fields.extend(form.fields.iter().cloned());
        }
        fields.into_iter().any(|input| {
            input.update(cx, |state, cx| {
                state.marked_text_range(window, cx).is_some()
            })
        })
    }
    pub(crate) fn query(&self, _cx: &App) -> core::Result<TaskQuery> {
        let day = time::calendar(self.options.day, &self.options.zone)?;
        let text = &self.search_text;
        Ok(TaskQuery {
            day,
            list: self.filters.list,
            status: self.filters.status,
            due: match self.filters.due {
                1 => DueFilter::ThroughSelectedDay,
                2 => DueFilter::Undated,
                _ => DueFilter::Any,
            },
            blocks: self.filters.blocks,
            title_contains: if text.trim().is_empty() {
                None
            } else {
                Some(SearchText::new(text)?)
            },
        })
    }
    fn read(
        &mut self,
        work: Work,
        context: ReadContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.client.request(work.clone()) {
            Ok(receiver) => {
                cx.spawn_in(window, async move |weak, cx| {
                    let response = receiver.await;
                    let _ = weak.update_in(cx, |this, window, cx| {
                        let start = Clock::now();
                        match response {
                            Ok(response) => {
                                this.diagnostics.sample(
                                    "response_receive",
                                    response.request_id,
                                    micros(response.finished.elapsed()),
                                    1,
                                );
                                this.receive(response.result, work, context, window, cx);
                                this.diagnostics.sample(
                                    "display_apply",
                                    response.request_id,
                                    micros(start.elapsed()),
                                    1,
                                );
                            }
                            Err(_) => this.receive(
                                Err(Failure::Local(
                                    "保存担当からの応答が途切れました。再起動してください".into(),
                                )),
                                work,
                                context,
                                window,
                                cx,
                            ),
                        }
                    });
                })
                .detach();
            }
            Err(error) => {
                self.receive(Err(Failure::Local(error.into())), work, context, window, cx)
            }
        }
    }
    fn read_finished(&mut self, context: &ReadContext) {
        match context {
            ReadContext::Tasks(..) => self.task_loading = false,
            ReadContext::Agenda(_, true, _) => self.all_day_loading = false,
            ReadContext::Agenda(_, false, _) => self.agenda_loading = false,
            _ => {}
        }
    }
    pub(crate) fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.status = message;
        self.error = true;
        cx.notify();
    }
    fn clear_read_error(&mut self, scope: ReadScope) {
        let showing_read_error = self
            .failed_reads
            .0
            .iter()
            .any(|(_, _, message)| *message == self.status);
        self.failed_reads.clear(scope);
        self.read_error = self.failed_reads.any();
        if showing_read_error {
            self.error = self.read_error;
            self.status = self
                .failed_reads
                .0
                .last()
                .map(|(_, _, message)| message.clone())
                .unwrap_or_else(|| "ローカルの内容を表示中".into());
        }
    }
    fn read_is_current(&self, context: &ReadContext) -> bool {
        match context {
            ReadContext::Tasks(epoch, _) | ReadContext::Rows(epoch) => *epoch == self.task_epoch,
            ReadContext::Agenda(epoch, true, _) => *epoch == self.all_day_epoch,
            ReadContext::Agenda(epoch, false, _) => *epoch == self.agenda_epoch,
            ReadContext::Detail(..)
            | ReadContext::ListDetail(..)
            | ReadContext::ExistingBlock(..)
            | ReadContext::Entity(..) => {
                context.selection_is_current(self.selection_epoch, self.selected, &self.tasks)
            }
            ReadContext::Conflict(stamp) => self
                .form
                .as_ref()
                .is_some_and(|form| form.stamp.accepts(*stamp)),
            _ => true,
        }
    }
    fn receive(
        &mut self,
        result: std::result::Result<Payload, Failure>,
        work: Work,
        context: ReadContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_finished(&context);
        if self.read_is_current(&context) {
            if result.is_ok() {
                self.clear_read_error(context.scope());
            }
            self.apply_read(result, work, context.clone(), window, cx);
        }
        match context {
            ReadContext::Tasks(..) => {
                if let Some((pos, mode)) = self.task_pending.take() {
                    self.query_tasks(pos, mode, window, cx);
                }
            }
            ReadContext::Agenda(_, true, _) if self.all_day_pending => {
                self.all_day_pending = false;
                self.query_agenda(true, false, window, cx);
            }
            ReadContext::Agenda(_, false, _) if self.agenda_pending => {
                self.agenda_pending = false;
                self.query_agenda(false, false, window, cx);
            }
            _ => {}
        }
        cx.notify();
    }
    fn apply_read(
        &mut self,
        result: std::result::Result<Payload, Failure>,
        work: Work,
        context: ReadContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(context, ReadContext::Recovery) {
            self.recovered(result, window, cx);
            return;
        }
        match result {
            Err(Failure::Core(Error::StalePage | Error::CursorMismatch)) => {
                match context {
                    ReadContext::Tasks(..) | ReadContext::Rows(..) => {
                        let pos = self
                            .tasks
                            .anchor(&self.task_list)
                            .map(|(id, _)| TaskPosition::Around(id))
                            .unwrap_or(TaskPosition::First);
                        self.query_tasks(pos, PageMode::Replace, window, cx);
                    }
                    ReadContext::Agenda(_, all_day, _) => {
                        self.query_agenda(all_day, false, window, cx);
                    }
                    ReadContext::Collections => {
                        self.load_collections(PagePosition::First, window, cx);
                    }
                    _ => self.fail(user_error(&Error::StalePage), cx),
                }
                return;
            }
            Err(error) => {
                match context {
                    ReadContext::Agenda(_, true, _) => self.all_day_loaded = false,
                    ReadContext::Agenda(_, false, _) => self.agenda_loaded = false,
                    _ => {}
                }
                let message = format!("読み込めませんでした。表示中の値は保持しています: {error}");
                self.failed_reads.record(work, context, message.clone());
                self.read_error = true;
                self.fail(message, cx);
                return;
            }
            Ok(Payload::Opened(opened)) => {
                self.info = Some(opened.info);
                self.history = opened.history;
                self.settings = opened.settings;
                let (t, motion) = self.settings.apply(cx);
                self.tokens = t;
                self.motion = motion;
                self.status = opened
                    .warning
                    .unwrap_or_else(|| "ローカルの内容を表示中".into());
                self.error = false;
                self.recovery = false;
                if let Some(target) = self
                    .failed_write
                    .as_ref()
                    .and_then(|failed| work_target(&failed.work))
                {
                    self.recovery = true;
                    self.status = "保存結果を確認しています…".into();
                    self.read(Work::Get(target), ReadContext::Recovery, window, cx);
                } else {
                    self.failed_write = None;
                }
                self.task_epoch += 1;
                self.query_tasks(TaskPosition::First, PageMode::Replace, window, cx);
                self.load_agenda(window, cx);
                self.load_collections(PagePosition::First, window, cx);
                if let Some(form) = &self.form
                    && self
                        .failed_write
                        .as_ref()
                        .is_some_and(|failed| failed.origin.matches_form(Some(form.stamp)))
                {
                    self.read(
                        Work::Get(form.draft.target.id()),
                        ReadContext::Conflict(form.stamp),
                        window,
                        cx,
                    );
                }
            }
            Ok(Payload::Tasks(page)) => {
                if let ReadContext::Tasks(epoch, mode) = context {
                    if epoch == self.task_epoch && page.generation.get() >= self.commit_generation {
                        self.tasks.accept(page, mode, &self.task_list);
                        self.selected = self.tasks.visible_selection(self.selected);
                        self.diagnostics
                            .sample("task_cache", 0, 0, self.tasks.rows.len() as u64);
                    } else if self.task_pending.is_none() {
                        self.task_pending = Some((
                            self.tasks
                                .anchor(&self.task_list)
                                .map(|(id, _)| TaskPosition::Around(id))
                                .unwrap_or(TaskPosition::First),
                            PageMode::Replace,
                        ));
                    }
                }
            }
            Ok(Payload::Rows(rows)) => {
                if let ReadContext::Rows(epoch) = context
                    && epoch == self.task_epoch
                    && rows.generation.get() >= self.commit_generation
                {
                    self.tasks.refresh(rows, &self.task_list);
                    self.selected = self.tasks.visible_selection(self.selected);
                    if self.tasks.loaded
                        && self.status == "ローカルに保存しました"
                        && self.saved_task.is_some_and(|id| {
                            !self.tasks.rows.iter().any(|row| row.task.key.id == id)
                        })
                    {
                        self.status = "保存しました。一覧の表示範囲外です。タスクを開けます".into();
                    }
                }
            }
            Ok(Payload::Agenda(page)) => {
                if let ReadContext::Agenda(epoch, all_day, append) = context
                    && epoch
                        == if all_day {
                            self.all_day_epoch
                        } else {
                            self.agenda_epoch
                        }
                    && page.generation.get() >= self.commit_generation
                {
                    if all_day {
                        self.all_day_loaded = true;
                    } else {
                        self.agenda_loaded = true;
                    }
                    if !all_day && !append {
                        self.moved_from.clear();
                        for item in &page.items {
                            if let Some(old) = self
                                .agenda
                                .iter()
                                .find(|old| time::id(old) == time::id(item))
                                && let (Some(before), Some(after)) =
                                    (time::span(old), time::span(item))
                                && before.bounds().0 != after.bounds().0
                            {
                                self.moved_from.insert(time::id(item), before.bounds().0);
                            }
                        }
                    }
                    let (items, total, next) = if all_day {
                        (
                            &mut self.all_day,
                            &mut self.all_day_total,
                            &mut self.all_day_next,
                        )
                    } else {
                        (
                            &mut self.agenda,
                            &mut self.agenda_total,
                            &mut self.agenda_next,
                        )
                    };
                    *total = page.total;
                    *next = page.next;
                    if append {
                        for item in page.items {
                            if !items.iter().any(|old| time::id(old) == time::id(&item)) {
                                items.push(item);
                            }
                        }
                        if items.len() > 400 {
                            items.drain(..items.len() - 400);
                        }
                    } else {
                        *items = page.items;
                    }
                }
            }
            Ok(Payload::Collections(page)) => {
                self.collections = page.items;
                self.collections_next = page.next;
                self.collections_previous = page.previous;
                if let Some(form) = &self.form {
                    let items = collection_options(&self.collections, &form.draft);
                    let selected = if matches!(form.draft.target, Target::Task { .. }) {
                        core::EntityId::List(form.draft.list)
                    } else {
                        core::EntityId::Calendar(form.draft.calendar)
                    };
                    form.collection.update(cx, |state, cx| {
                        state.set_items(items, window, cx);
                        state.set_selected_value(&selected, window, cx);
                    });
                }
            }
            Ok(Payload::Detail(detail)) => {
                if let ReadContext::ExistingBlock(epoch, id) = context {
                    if epoch == self.selection_epoch
                        && let Some(info) = &self.info
                    {
                        match Draft::existing_block(
                            detail,
                            id,
                            self.options.day,
                            &self.options.zone,
                            info,
                        ) {
                            Ok(draft) => self.set_form(draft, window, cx),
                            Err(error) => self.fail(user_error(&error), cx),
                        }
                    }
                } else if let ReadContext::Detail(epoch, allocate)
                | ReadContext::ListDetail(epoch, _, allocate) = context
                    && epoch == self.selection_epoch
                {
                    let Some(info) = self.info.as_ref() else {
                        return;
                    };
                    if allocate && detail.blocks.is_empty() {
                        match TimeBlockId::new() {
                            Ok(id) => {
                                let task = TaskSummary {
                                    key: detail.task.key,
                                    title: detail.task.data.title.clone(),
                                    status: detail.task.data.status,
                                };
                                let mut draft = Draft::blank(
                                    Target::Block {
                                        id,
                                        expected: None,
                                        task,
                                    },
                                    self.options.day,
                                    info,
                                );
                                draft.title = detail.task.data.title.as_str().into();
                                self.set_form(draft, window, cx);
                            }
                            Err(e) => self.fail(user_error(&e), cx),
                        }
                    } else if allocate && detail.blocks.len() == 1 {
                        let task = TaskSummary {
                            key: detail.task.key,
                            title: detail.task.data.title.clone(),
                            status: detail.task.data.status,
                        };
                        let draft = Draft::block(
                            detail.blocks[0].clone(),
                            task,
                            self.options.day,
                            &self.options.zone,
                            info,
                        );
                        self.set_form(draft, window, cx);
                    } else {
                        let draft = Draft::task(detail, self.options.day, &self.options.zone, info);
                        self.set_form(draft, window, cx);
                    }
                }
            }
            Ok(Payload::Entity(entity)) => {
                if let ReadContext::Conflict(stamp) = context {
                    if let Some(form) = &mut self.form
                        && form.stamp.accepts(stamp)
                    {
                        form.conflict = Some(entity_summary(&entity, &self.options.zone));
                    }
                } else if let ReadContext::Entity(epoch) = context
                    && epoch == self.selection_epoch
                    && let Some(info) = self.info.as_ref()
                {
                    let draft = match entity {
                        core::Entity::Event(event) => {
                            Draft::event(event, self.options.day, &self.options.zone, info)
                        }
                        core::Entity::TimeBlock(block) => {
                            self.selected = Some(block.task);
                            self.read(
                                Work::Detail(block.task),
                                ReadContext::ExistingBlock(epoch, block.key.id),
                                window,
                                cx,
                            );
                            return;
                        }
                        core::Entity::List(list) => {
                            let mut draft = Draft::blank(
                                Target::List {
                                    id: list.key.id,
                                    expected: Some(list.key),
                                },
                                self.options.day,
                                info,
                            );
                            draft.title = list.data.as_str().into();
                            draft
                        }
                        core::Entity::Calendar(calendar) => {
                            let mut draft = Draft::blank(
                                Target::Calendar {
                                    id: calendar.key.id,
                                    expected: Some(calendar.key),
                                },
                                self.options.day,
                                info,
                            );
                            draft.title = calendar.data.as_str().into();
                            draft
                        }
                        core::Entity::Task(task) => {
                            self.read(
                                Work::Detail(task.key.id),
                                ReadContext::Detail(epoch, false),
                                window,
                                cx,
                            );
                            return;
                        }
                    };
                    self.set_form(draft, window, cx);
                }
            }
            Ok(Payload::Settings(settings)) => {
                self.settings = settings;
                let (t, motion) = self.settings.apply(cx);
                self.tokens = t;
                self.motion = motion;
                self.clear_popup(window, cx);
                self.status = "表示設定を保存しました".into();
                self.restore_focus(window, cx);
            }
            Ok(Payload::Saved(_)) => {}
        }
        if !self.first_data_recorded
            && self.tasks.loaded
            && self.agenda_loaded
            && self.all_day_loaded
        {
            self.first_data_recorded = true;
            self.diagnostics.sample("first_data_applied", 0, 0, 1);
        }
        cx.notify();
    }
    fn invalidate_task_scope(&mut self) {
        self.task_epoch += 1;
        self.selection_epoch += 1;
        self.selected = None;
        self.focus_editor_input = false;
        self.task_pending = None;
        self.search_timer = None;
        self.tasks.invalidate();
        self.clear_read_error(ReadScope::Tasks);
        self.clear_read_error(ReadScope::Rows);
        self.clear_read_error(ReadScope::Selection);
    }
    pub(crate) fn apply_filters(
        &mut self,
        filters: Filters,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.filters != filters {
            self.filters = filters;
            self.invalidate_task_scope();
            self.query_tasks(TaskPosition::First, PageMode::Replace, window, cx);
        }
        cx.notify();
    }
    pub(crate) fn query_tasks(
        &mut self,
        position: TaskPosition,
        mode: PageMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.info.is_none() {
            return;
        }
        if self.task_loading {
            self.task_pending = Some((position, mode));
            return;
        }
        match self
            .query(cx)
            .and_then(|query| Ok((query, PageSize::new(200)?)))
        {
            Ok((query, size)) => {
                self.task_loading = true;
                self.read(
                    Work::Tasks(query, TaskPageRequest { size, position }),
                    ReadContext::Tasks(self.task_epoch, mode),
                    window,
                    cx,
                );
            }
            Err(error) => self.fail(user_error(&error), cx),
        }
    }
    pub(crate) fn page_tasks(
        &mut self,
        previous: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tasks.loaded || self.task_loading {
            return;
        }
        let position = if previous {
            self.tasks
                .previous
                .clone()
                .map(TaskPosition::Before)
                .or_else(|| {
                    self.tasks
                        .rows
                        .first()
                        .map(|r| TaskPosition::Around(r.task.key.id))
                })
        } else {
            self.tasks
                .next
                .clone()
                .map(TaskPosition::After)
                .or_else(|| {
                    self.tasks
                        .rows
                        .last()
                        .map(|r| TaskPosition::Around(r.task.key.id))
                })
        };
        if let Some(pos) = position {
            self.query_tasks(
                pos,
                if previous {
                    PageMode::Previous
                } else {
                    PageMode::Next
                },
                window,
                cx,
            );
        }
    }
    fn initial_scroll(&self) {
        if let (Ok(day), Ok(local)) = (
            time::calendar(self.options.day, &self.options.zone),
            LocalDateTime::parse(&format!("{}T09:00:00", self.options.day.iso())),
        ) && let Ok(at) = resolve_local(local, &self.options.zone, OverlapChoice::Earlier)
        {
            let offset = (at.instant.unix_micros() - day.instant_bounds().0.unix_micros()) as f32
                / time::MINUTE as f32;
            self.agenda_scroll
                .set_offset(point(px(0.), px(-time::initial_axis_offset(offset))));
        }
    }
    pub(crate) fn load_agenda(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agenda_epoch += 1;
        self.all_day_epoch += 1;
        self.query_agenda(false, false, window, cx);
        self.query_agenda(true, false, window, cx);
    }
    pub(crate) fn query_agenda(
        &mut self,
        all_day: bool,
        append: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.info.is_none() {
            return;
        }
        if (all_day && self.all_day_loading) || (!all_day && self.agenda_loading) {
            if all_day {
                self.all_day_pending = true;
            } else {
                self.agenda_pending = true;
            }
            return;
        }
        let result = (|| -> core::Result<_> {
            let day = time::calendar(self.options.day, &self.options.zone)?;
            let range = if all_day {
                AgendaRange::AllDay(day.days())
            } else {
                let (base, end) = day.instant_bounds();
                let top = (-f32::from(self.agenda_scroll.offset().y)).max(0.);
                let height = f32::from(window.viewport_size().height).max(200.);
                let start = core::Instant::from_unix_micros(
                    (base.unix_micros() + ((top - 90.).max(0.) as i64) * time::MINUTE)
                        .min(end.unix_micros()),
                )?;
                let stop = core::Instant::from_unix_micros(
                    (base.unix_micros() + ((top + height + 90.) as i64) * time::MINUTE)
                        .min(end.unix_micros()),
                )?;
                let timed = if append {
                    self.agenda_range.clone().unwrap_or(day.clip(start, stop)?)
                } else {
                    day.clip(start, stop)?
                };
                self.agenda_range = Some(timed.clone());
                AgendaRange::Timed(timed)
            };
            let next = if all_day {
                self.all_day_next.clone()
            } else {
                self.agenda_next.clone()
            };
            let position = if append {
                next.map(PagePosition::After).unwrap_or(PagePosition::First)
            } else {
                PagePosition::First
            };
            Ok(Work::Agenda(
                AgendaQuery {
                    range,
                    calendar: None,
                },
                PageRequest {
                    size: PageSize::new(200)?,
                    position,
                },
            ))
        })();
        match result {
            Ok(work) => {
                if all_day {
                    self.all_day_loading = true;
                } else {
                    self.agenda_loading = true;
                }
                self.read(
                    work,
                    ReadContext::Agenda(
                        if all_day {
                            self.all_day_epoch
                        } else {
                            self.agenda_epoch
                        },
                        all_day,
                        append,
                    ),
                    window,
                    cx,
                );
            }
            Err(e) => self.fail(user_error(&e), cx),
        }
    }
    pub(crate) fn agenda_scrolled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agenda_epoch += 1;
        self.query_agenda(false, false, window, cx);
        cx.notify();
    }
    pub(crate) fn load_collections(
        &mut self,
        position: PagePosition<CollectionCursor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match PageSize::new(200) {
            Ok(size) => self.read(
                Work::Collections(PageRequest { size, position }),
                ReadContext::Collections,
                window,
                cx,
            ),
            Err(e) => self.fail(user_error(&e), cx),
        }
    }
    pub(crate) fn add_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.recovery || self.composing(window, cx) {
            return;
        }
        let Some(info) = &self.info else {
            return;
        };
        let title = self.quick.read(cx).value().to_string();
        let result = (|| -> core::Result<_> {
            let id = match self.quick_id {
                Some(id) => id,
                None => TaskId::new()?,
            };
            let data = TaskData {
                title: Title::new(&title)?,
                list: self.quick_list.or(self.filters.list).unwrap_or(info.default_list),
                status: TaskStatus::Open,
                due: None,
            };
            Ok((id, Work::SaveTask(Save::Create { id, data })))
        })();
        match result {
            Ok((id, work)) => {
                self.quick_id = Some(id);
                self.quick_validation = None;
                self.write(
                    work,
                    WriteOrigin::Quick {
                        title,
                        version: self.quick_version,
                    },
                    window,
                    cx,
                );
            }
            Err(e) => {
                let message = user_error(&e);
                if matches!(e, Error::InvalidInput { .. }) {
                    self.quick_validation = Some(message.clone());
                }
                self.fail(message, cx);
            }
        }
    }
    pub(crate) fn write(
        &mut self,
        work: Work,
        origin: WriteOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy
            || self.recovery
            || (matches!(origin, WriteOrigin::Form { .. })
                && !origin.matches_form(self.form.as_ref().map(|f| f.stamp)))
        {
            return;
        }
        match self.client.request(work.clone()) {
            Ok(receiver) => {
                self.busy = true;
                self.error = false;
                self.failed_write = None;
                self.status = "保存中…".into();
                self.pending_write = Some(PendingWrite { work, origin });
                cx.spawn_in(window, async move |weak, cx| {
                    let result = receiver.await;
                    let _ = weak.update_in(cx, |this, window, cx| {
                        this.busy = false;
                        match result {
                            Ok(response) => {
                                this.diagnostics.sample(
                                    "response_receive",
                                    response.request_id,
                                    micros(response.finished.elapsed()),
                                    1,
                                );
                                match response.result {
                                    Ok(Payload::Saved(commit)) => {
                                        this.committed(commit, window, cx)
                                    }
                                    Err(error) => this.write_failed(error, window, cx),
                                    _ => this.write_failed(
                                        Failure::Core(Error::RecoveryRequired),
                                        window,
                                        cx,
                                    ),
                                }
                            }
                            Err(_) => this.write_failed(
                                Failure::Core(Error::RecoveryRequired),
                                window,
                                cx,
                            ),
                        }
                    });
                })
                .detach();
                cx.notify();
            }
            Err(error) => {
                self.failed_write = Some(PendingWrite { work, origin });
                self.after_save = None;
                self.quit_after_save = false;
                self.fail(error.into(), cx);
            }
        }
    }
    fn write_failed(&mut self, error: Failure, window: &mut Window, cx: &mut Context<Self>) {
        self.failed_write = self.pending_write.take();
        self.after_save = None;
        self.quit_after_save = false;
        if let Failure::Core(
            error @ (Error::InvalidInput { .. }
            | Error::NonexistentLocalTime { .. }
            | Error::AmbiguousLocalTime { .. }),
        ) = &error
        {
            if let Some(form) = &mut self.form
                && self
                    .failed_write
                    .as_ref()
                    .is_some_and(|w| w.origin.matches_form(Some(form.stamp)))
            {
                form.validation = Some(Validation::from_error(error, form.draft.all_day));
            }
            self.failed_write = None;
            self.fail(user_error(error), cx);
            return;
        }
        if matches!(
            error,
            Failure::Core(
                Error::RecoveryRequired
                    | Error::Storage {
                        outcome: CommitKnowledge::Unknown,
                        ..
                    }
            )
        ) {
            self.recovery = true;
        }
        if let Failure::Core(Error::Conflict { entity, .. } | Error::HistoryConflict { entity }) =
            &error
        {
            if let Some(form) = &mut self.form
                && self
                    .failed_write
                    .as_ref()
                    .is_some_and(|w| w.origin.matches_form(Some(form.stamp)))
            {
                form.conflict = Some("最新の内容を取得しています…".into());
                let stamp = form.stamp;
                self.read(Work::Get(*entity), ReadContext::Conflict(stamp), window, cx);
            }
            self.failed_write = None;
        }
        self.fail(
            format!(
                "{}。入力は残っています: {error}",
                if self.recovery {
                    "保存結果を確認しています"
                } else {
                    "保存できませんでした"
                }
            ),
            cx,
        );
    }
    fn committed(&mut self, commit: Commit<()>, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_generation = self.commit_generation.max(commit.generation.get());
        self.history = commit.history;
        self.error = false;
        self.status = "ローカルに保存しました".into();
        self.saved_task = None;
        let mut ids = Vec::new();
        let mut agenda_changed = false;
        let mut collections_changed = false;
        for change in &commit.changed {
            match change {
                Change::Upsert(core::Entity::Task(task)) => {
                    self.saved_task = Some(task.key.id);
                    ids.push(task.key.id);
                    for item in &mut self.agenda {
                        if let AgendaItem::TimeBlock { task: summary, .. } = item
                            && summary.key.id == task.key.id
                        {
                            *summary = TaskSummary {
                                key: task.key,
                                title: task.data.title.clone(),
                                status: task.data.status,
                            };
                        }
                    }
                }
                Change::Upsert(core::Entity::TimeBlock(block)) => {
                    ids.push(block.task);
                    agenda_changed = true;
                }
                Change::Upsert(core::Entity::Event(_)) => agenda_changed = true,
                Change::Upsert(core::Entity::List(list)) => {
                    collections_changed = true;
                    for (index, row) in self.tasks.rows.iter_mut().enumerate() {
                        if row.task.data.list == list.key.id {
                            row.list_title = list.data.clone();
                            self.task_list.splice(index..index + 1, 1);
                        }
                    }
                }
                Change::Upsert(core::Entity::Calendar(_)) => collections_changed = true,
                Change::Deleted {
                    id: core::EntityId::Task(id),
                    ..
                } => {
                    self.tasks.delete(*id, &self.task_list);
                    ids.push(*id);
                    agenda_changed = true;
                }
                Change::Deleted {
                    id: core::EntityId::Event(_) | core::EntityId::TimeBlock(_),
                    ..
                } => agenda_changed = true,
                Change::Deleted { .. } => collections_changed = true,
            }
        }
        ids.sort();
        ids.dedup();
        if !ids.is_empty()
            && let Ok(query) = self.query(cx)
        {
            self.read(
                Work::Rows(query, ids),
                ReadContext::Rows(self.task_epoch),
                window,
                cx,
            );
        }
        if agenda_changed {
            self.load_agenda(window, cx);
        }
        if collections_changed {
            self.load_collections(PagePosition::First, window, cx);
        }
        if let Some(pending) = self.pending_write.take() {
            match pending.origin {
                WriteOrigin::Quick { title, version } => {
                    self.quick_id = None;
                    if version == self.quick_version
                        && self.quick.read(cx).value().as_ref() == title
                    {
                        self.quick_validation = None;
                        self.quick_list = None;
                        self.quick.update(cx, |input, cx| {
                            input.set_value("", window, cx);
                        });
                        if matches!(self.popup, Some(Popup::QuickTask)) {
                            self.dismiss_popup(window, cx);
                        }
                    }
                }
                WriteOrigin::Form { stamp } => {
                    if self.form.as_ref().is_some_and(|f| f.stamp.accepts(stamp)) {
                        self.discard_form();
                        self.restore_focus(window, cx);
                    } else {
                        self.after_save = None;
                        self.quit_after_save = false;
                    }
                }
                WriteOrigin::Direct => {}
            }
        }
        if let Some(nav) = self.after_save.take() {
            if matches!(nav, Navigation::Quit) && self.dirty(cx) {
                self.open_popup(Popup::Leave(nav), window, cx);
            } else {
                self.navigate_now(nav, window, cx);
            }
        }
        if self.quit_after_save {
            self.quit_after_save = false;
            if self.dirty(cx) {
                self.open_popup(Popup::Leave(Navigation::Quit), window, cx);
            } else {
                self.allow_close = true;
                window.remove_window();
            }
        }
        self.diagnostics
            .sample("commit_applied", 0, 0, commit.changed.len() as u64);
        cx.notify();
    }
    pub(crate) fn toggle_task(&mut self, id: TaskId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tasks.visible_selection(Some(id)).is_none() {
            return;
        }
        if let Some(row) = self.tasks.rows.iter().find(|row| row.task.key.id == id) {
            let mut data = row.task.data.clone();
            data.status = if data.status == TaskStatus::Open {
                TaskStatus::Done
            } else {
                TaskStatus::Open
            };
            self.write(
                Work::SaveTask(Save::Update {
                    expected: row.task.key,
                    data,
                }),
                WriteOrigin::Direct,
                window,
                cx,
            );
        }
    }
    pub(crate) fn history_action(
        &mut self,
        redo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.form.as_ref().is_some_and(|f| f.dirty) {
            self.fail(
                "編集中の変更を保存または取り消してから履歴を操作してください".into(),
                cx,
            );
            return;
        }
        if redo {
            if let Some(entry) = self.history.redo.clone() {
                self.write(Work::Redo(entry.target), WriteOrigin::Direct, window, cx);
            }
        } else if let Some(entry) = self.history.undo.clone() {
            self.write(Work::Undo(entry.target), WriteOrigin::Direct, window, cx);
        }
    }
    pub(crate) fn navigate(
        &mut self,
        nav: Navigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !nav.is_available(&self.tasks) {
            return;
        }
        if self.form.as_ref().is_some_and(|f| f.dirty) || self.busy {
            self.open_popup(Popup::Leave(nav), window, cx);
            return;
        }
        self.navigate_now(nav, window, cx);
    }
    pub(crate) fn navigate_now(
        &mut self,
        nav: Navigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !nav.is_available(&self.tasks) {
            return;
        }
        self.clear_popup(window, cx);
        self.drag = None;
        self.selection_epoch += 1;
        self.clear_read_error(ReadScope::Selection);
        self.discard_form();
        match nav {
            Navigation::QuickInput => {
                if self.quick_list.is_none() {
                    self.quick_list = self.filters.list.or_else(|| self.info.as_ref().map(|info| info.default_list));
                }
                self.open_popup(Popup::QuickTask, window, cx);
                self.quick.update(cx, |input, cx| input.focus(window, cx));
            }
            Navigation::SearchInput => {
                self.calendar_view = false;
                self.search_open = true;
                self.compact_agenda = false;
                self.search.update(cx, |input, cx| input.focus(window, cx));
            }
            Navigation::Tasks => {
                self.calendar_view = false;
                self.compact_agenda = false;
                self.restore_focus(window, cx);
            }
            Navigation::Calendar => {
                self.calendar_view = true;
                self.compact_agenda = true;
                self.focus.focus(window, cx);
            }
            Navigation::List(list) => {
                self.calendar_view = false;
                self.compact_agenda = false;
                self.apply_filters(
                    Filters {
                        list,
                        ..self.filters.clone()
                    },
                    window,
                    cx,
                );
                self.list_focus.focus(window, cx);
            }
            Navigation::Day(day) => {
                let position = self
                    .tasks
                    .anchor(&self.task_list)
                    .map(|(id, _)| TaskPosition::Around(id))
                    .unwrap_or(TaskPosition::First);
                if self.options.day != day {
                    self.invalidate_task_scope();
                    self.agenda.clear();
                    self.all_day.clear();
                    self.agenda_loaded = false;
                    self.all_day_loaded = false;
                    self.agenda_total = 0;
                    self.all_day_total = 0;
                } else {
                    self.task_epoch += 1;
                }
                self.options.day = day;
                self.date_input
                    .update(cx, |input, cx| input.set_value(day.iso(), window, cx));
                self.query_tasks(position, PageMode::Replace, window, cx);
                self.load_agenda(window, cx);
            }
            Navigation::Task(id) | Navigation::ListTask(id, _) => {
                self.selected = Some(id);
                let context = match nav {
                    Navigation::ListTask(_, allocate) => {
                        ReadContext::ListDetail(self.selection_epoch, id, allocate)
                    }
                    _ => ReadContext::Detail(self.selection_epoch, false),
                };
                self.read(Work::Detail(id), context, window, cx);
            }
            Navigation::Event => {
                if let Some(info) = &self.info {
                    match EventId::new() {
                        Ok(id) => {
                            let draft = Draft::blank(
                                Target::Event { id, expected: None },
                                self.options.day,
                                info,
                            );
                            self.set_form(draft, window, cx);
                        }
                        Err(e) => self.fail(user_error(&e), cx),
                    }
                }
            }
            Navigation::Entity(id) => self.read(
                Work::Get(id),
                ReadContext::Entity(self.selection_epoch),
                window,
                cx,
            ),
            Navigation::Collection(calendar) => {
                if let Some(info) = &self.info {
                    let target = if calendar {
                        CalendarId::new()
                            .map(|id| (Target::Calendar { id, expected: None }, String::new()))
                    } else {
                        ListId::new().map(|id| (Target::List { id, expected: None }, String::new()))
                    };
                    match target {
                        Ok((target, title)) => {
                            let mut draft = Draft::blank(target, self.options.day, info);
                            draft.title = title;
                            self.set_form(draft, window, cx);
                        }
                        Err(e) => self.fail(user_error(&e), cx),
                    }
                }
            }
            Navigation::Quit => {
                self.allow_close = true;
                window.remove_window();
            }
        }
        cx.notify();
    }
    pub(crate) fn discard_form(&mut self) {
        let had_validation = self
            .form
            .as_ref()
            .is_some_and(|form| form.validation.is_some());
        self.form = None;
        self.clear_read_error(ReadScope::Conflict);
        let abandoned = PendingWrite::abandon_form(&mut self.failed_write, self.recovery);
        if !self.recovery && (abandoned || had_validation) {
            self.error = false;
            self.status = "変更を破棄しました".into();
        }
    }
    pub(crate) fn retryable_write(&self) -> bool {
        self.failed_write
            .as_ref()
            .is_some_and(|w| w.can_retry(self.form.as_ref().map(|f| f.stamp)))
    }
    pub(crate) fn set_form(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.is_none() {
            self.return_focus = window.focused(cx);
        }
        self.discard_form();
        self.form_serial += 1;
        let stamp = FormStamp {
            serial: self.form_serial,
            version: 0,
        };
        let collection_items = collection_options(&self.collections, &draft);
        let selected_collection = if matches!(draft.target, Target::Task { .. }) {
            core::EntityId::List(draft.list)
        } else {
            core::EntityId::Calendar(draft.calendar)
        };
        let selected_index = collection_items
            .iter()
            .position(|item| item.id == selected_collection)
            .map(gpui_kit::component::IndexPath::new);
        let collection = cx.new(|cx| {
            gpui_kit::component::select::SelectState::new(
                collection_items,
                selected_index,
                window,
                cx,
            )
        });
        let fields = [
            &draft.start_date,
            &draft.start_time,
            &draft.end_date,
            &draft.end_time,
        ]
        .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value.clone())));
        let title_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .default_value(draft.title.clone())
                .submit_on_enter(true)
        });
        let mut subscriptions: Vec<_> = fields
            .iter()
            .map(|input| {
                cx.subscribe_in(input, window, move |this, _, event, window, cx| {
                    if this
                        .form
                        .as_ref()
                        .is_none_or(|f| f.stamp.serial != stamp.serial)
                    {
                        return;
                    }
                    let start = Clock::now();
                    if matches!(event, InputEvent::Change) {
                        if this
                            .form
                            .as_ref()
                            .is_some_and(|f| !f.dirty || f.validation.is_some())
                            || this.failed_write.is_some()
                        {
                            cx.notify();
                        }
                        if let Some(form) = &mut this.form {
                            form.changed();
                            form.submit_ready = false;
                        }
                        this.diagnostics
                            .sample("input_handler_cpu", 0, micros(start.elapsed()), 1);
                    } else if matches!(
                        event,
                        InputEvent::PressEnter {
                            secondary: true,
                            ..
                        }
                    ) {
                        this.save_from_keyboard(window, cx);
                    } else if matches!(event, InputEvent::Blur) {
                        cx.notify();
                    }
                })
            })
            .collect();
        subscriptions.push(cx.subscribe_in(
            &title_input,
            window,
            move |this, _, event, window, cx| {
                match event {
                    _ if this
                        .form
                        .as_ref()
                        .is_none_or(|f| f.stamp.serial != stamp.serial) => {}
                    InputEvent::Change => {
                        let start = Clock::now();
                        if this
                            .form
                            .as_ref()
                            .is_some_and(|f| !f.dirty || f.validation.is_some())
                            || this.failed_write.is_some()
                        {
                            cx.notify();
                        }
                        if let Some(form) = &mut this.form {
                            form.changed();
                            form.submit_ready = false;
                        }
                        this.diagnostics
                            .sample("input_handler_cpu", 0, micros(start.elapsed()), 1);
                    }
                    InputEvent::PressEnter {
                        secondary: true, ..
                    } => {
                        this.save_from_keyboard(window, cx);
                    }
                    _ => {}
                }
            },
        ));
        subscriptions.push(
            cx.subscribe_in(&collection, window, move |this, _, event, _, cx| {
                if let gpui_kit::component::select::SelectEvent::Confirm(Some(id)) = event
                    && let Some(form) = &mut this.form
                    && form.stamp.serial == stamp.serial
                {
                    match id {
                        core::EntityId::List(id) => form.draft.list = *id,
                        core::EntityId::Calendar(id) => form.draft.calendar = *id,
                        _ => return,
                    }
                    form.changed();
                    cx.notify();
                }
            }),
        );
        self.form = Some(Form {
            draft,
            title_input,
            fields,
            collection,
            dirty: false,
            stamp,
            validation: None,
            conflict: None,
            _subscriptions: subscriptions,
            submit_ready: true,
            drag_candidate: false,
        });
        self.compact_agenda = false;
        if self.focus_editor_input {
            self.focus_editor_input = false;
            if let Some(form) = &self.form {
                form.title_input
                    .update(cx, |input, cx| input.focus(window, cx));
            }
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }
    pub(crate) fn save_from_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form {
            if !form.submit_ready {
                return;
            }
            form.submit_ready = false;
            self.save_form(window, cx);
        } else if self.quick_ready {
            self.quick_ready = false;
            self.add_task(window, cx);
        }
    }
    pub(crate) fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.recovery || self.composing(window, cx) {
            return;
        }
        let Some(form) = &mut self.form else {
            self.add_task(window, cx);
            return;
        };
        if form.conflict.is_some() {
            self.fail(
                "最新の内容を確認してください。下書きはコピーできます".into(),
                cx,
            );
            return;
        }
        let values: Vec<_> = form
            .fields
            .iter()
            .map(|field| field.read(cx).value().to_string())
            .collect();
        form.draft.title = form.title_input.read(cx).value().to_string();
        form.draft.start_date = values[0].clone();
        form.draft.start_time = values[1].clone();
        form.draft.end_date = values[2].clone();
        form.draft.end_time = values[3].clone();
        let stamp = form.stamp;
        form.validation = None;
        let result = form.draft.command(&self.options.zone);
        match result {
            Ok(work) => self.write(work, WriteOrigin::Form { stamp }, window, cx),
            Err(error) => {
                let validation = Validation::from_error(&error, form.draft.all_day);
                self.status = validation.message.clone();
                self.error = true;
                form.validation = Some(validation);
                PendingWrite::abandon_form(&mut self.failed_write, self.recovery);
                self.after_save = None;
                self.quit_after_save = false;
                cx.notify();
            }
        }
    }
    pub(crate) fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.return_focus
            .take()
            .unwrap_or_else(|| self.list_focus.clone())
            .focus(window, cx);
    }
    pub(crate) fn open_popup(&mut self, popup: Popup, window: &mut Window, cx: &mut Context<Self>) {
        let resume_quick = matches!(popup, Popup::Leave(_))
            && (matches!(self.popup, Some(Popup::QuickTask)) || self.popup_resume_quick);
        self.clear_popup(window, cx);
        self.popup_resume_quick = resume_quick;
        let anchored = matches!(popup, Popup::Filters(_))
            || matches!(popup, Popup::Date) && self.calendar_view && self.form.is_none();
        self.popup = Some(popup);
        if !anchored {
            self.dialog_open = true;
            let weak = cx.weak_entity();
            window.open_dialog(cx, move |dialog, window, cx| {
                let current = weak.update(cx, |this, cx| {
                    (this.popup_content(window, cx), matches!(this.popup, Some(Popup::QuickTask)))
                }).ok();
                let (content, quick) = current.unwrap_or((None, false));
                let cancel = weak.clone();
                let closed = weak.clone();
                dialog
                    .centered()
                    .w(px(if quick { 750. } else { 400. }))
                    .rounded(px(if quick { 21. } else { 12. }))
                    .font(crate::theme::ui_font())
                    .overlay_closable(false)
                    .entrance(gpui_kit::component::dialog::DialogEntrance::None)
                    .on_ok(|_, _, _| false)
                    .on_cancel(move |_, window, cx| {
                        cancel.update(cx, |this, cx| {
                            if !this.composing(window, cx) {
                                this.dismiss_popup(window, cx);
                            }
                            false
                        }).unwrap_or(true)
                    })
                    .on_close(move |_, _, cx| {
                        let _ = closed.update(cx, |this, cx| {
                            this.popup = None;
                            this.dialog_open = false;
                            this.popup_resume_quick = false;
                            this.date_validation = None;
                            cx.notify();
                        });
                    })
                    .children(content)
            });
        }
        cx.notify();
    }
    pub(crate) fn clear_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popup = None;
        self.popup_resume_quick = false;
        if self.dialog_open {
            self.dialog_open = false;
            window.close_dialog(cx);
        }
    }
    pub(crate) fn dismiss_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let resume_quick = self.popup_resume_quick && matches!(self.popup, Some(Popup::Leave(_)));
        if matches!(self.popup, Some(Popup::Date)) {
            self.date_validation = None;
        }
        self.clear_popup(window, cx);
        if resume_quick {
            self.open_popup(Popup::QuickTask, window, cx);
            self.quick.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }
    pub(crate) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing(window, cx) {
            return;
        }
        if self.popup.is_some() {
            self.dismiss_popup(window, cx);
        } else if self.drag.take().is_some() {
            self.status = "仮配置を取り消しました".into();
            self.restore_focus(window, cx);
        } else if !self.busy && self.form.as_ref().is_some_and(|form| form.drag_candidate) {
            self.discard_form();
            self.status = "仮配置を取り消しました".into();
            self.restore_focus(window, cx);
        } else if self.form.is_some() {
            let destination = if self.calendar_view { Navigation::Calendar } else { Navigation::Tasks };
            self.navigate(destination, window, cx);
        } else {
            self.list_focus.focus(window, cx);
        }
        cx.notify();
    }
    pub(crate) fn select_relative(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tasks.loaded || self.tasks.rows.is_empty() {
            return;
        }
        let last = self.tasks.rows.len() - 1;
        let index = self
            .selected
            .and_then(|id| self.tasks.rows.iter().position(|r| r.task.key.id == id))
            .map(|index| index.saturating_add_signed(delta).min(last))
            .unwrap_or(if delta < 0 { last } else { 0 });
        self.selected = Some(self.tasks.rows[index].task.key.id);
        self.task_list.scroll_to_reveal_item(index);
        self.list_focus.focus(window, cx);
        cx.notify();
    }
    pub(crate) fn change_day(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        match self.options.day.add_days(delta) {
            Ok(day) => self.navigate(Navigation::Day(day), window, cx),
            Err(e) => self.fail(user_error(&e), cx),
        }
    }
    pub(crate) fn start_drag(
        &mut self,
        source: DragSource,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.calendar_view
            && self.form.is_none()
            && !self.busy
            && (f32::from(window.viewport_size().width) >= 900. || self.compact_agenda)
        {
            self.return_focus = window.focused(cx);
            self.drag = Some(Drag {
                source,
                origin: position,
                active: false,
                minute: None,
            });
        }
    }
    pub(crate) fn drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.calendar_view
            || self.form.is_some()
            || (f32::from(window.viewport_size().width) < 900. && !self.compact_agenda)
        {
            self.drag = None;
            return;
        }
        let Some(drag) = &mut self.drag else {
            return;
        };
        if !event.dragging() {
            self.drag = None;
            return;
        }
        let delta = event.position - drag.origin;
        if f32::from(delta.x).hypot(f32::from(delta.y)) >= 4. {
            drag.active = true;
        }
        let bounds = self.agenda_scroll.bounds();
        drag.minute = if drag.active && bounds.contains(&event.position) {
            Some(time::snap_minute(f32::from(
                event.position.y - bounds.top() - self.agenda_scroll.offset().y,
            )))
        } else {
            None
        };
        if drag.active && bounds.contains(&event.position) {
            let y = f32::from(event.position.y - bounds.top());
            let height = f32::from(bounds.size.height);
            let offset = self.agenda_scroll.offset();
            if y < 24. {
                self.agenda_scroll
                    .set_offset(point(offset.x, (offset.y + px(12.)).min(px(0.))));
            } else if y > height - 24. {
                self.agenda_scroll
                    .set_offset(point(offset.x, offset.y - px(12.)));
            }
        }
        let _ = window;
        cx.notify();
    }
    pub(crate) fn finish_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        self.suppress_click = true;
        cx.defer_in(window, |this, _, _| this.suppress_click = false);
        let Some(minute) = drag.minute else {
            self.restore_focus(window, cx);
            cx.notify();
            return;
        };
        let result = (|| -> core::Result<Draft> {
            let day = time::calendar(self.options.day, &self.options.zone)?;
            let (base, end) = day.instant_bounds();
            let start =
                core::Instant::from_unix_micros(base.unix_micros() + minute * time::MINUTE)?;
            if start >= end {
                return Err(Error::InvalidInput {
                    field: Field::Range,
                    problem: InputProblem::OutsideWindow,
                });
            }
            let info = self.info.as_ref().ok_or(Error::RecoveryRequired)?;
            let (mut draft, duration) = match drag.source {
                DragSource::Task(task) => {
                    let title = task.title.as_str().to_owned();
                    let mut d = Draft::blank(
                        Target::Block {
                            id: TimeBlockId::new()?,
                            expected: None,
                            task,
                        },
                        self.options.day,
                        info,
                    );
                    d.title = title;
                    (d, 30 * time::MINUTE)
                }
                DragSource::Block(block, task) => {
                    let (a, b) = block.data.span.bounds();
                    let duration = b.unix_micros() - a.unix_micros();
                    (
                        Draft::block(block, task, self.options.day, &self.options.zone, info),
                        duration,
                    )
                }
            };
            let span = ZonedSpan::new(
                start,
                core::Instant::from_unix_micros(start.unix_micros() + duration)?,
                self.options.zone.clone(),
            )?;
            draft.set_span(&span, &self.options.zone);
            Ok(draft)
        })();
        match result {
            Ok(draft) => {
                self.set_form(draft, window, cx);
                if let Some(form) = &mut self.form {
                    form.dirty = true;
                    form.drag_candidate = true;
                }
                self.status = "仮配置です。時刻を確認して保存してください".into();
            }
            Err(e) => self.fail(user_error(&e), cx),
        }
        cx.notify();
    }
    fn schedule_clock(&mut self, window: &Window, cx: &mut Context<Self>) {
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        self.now =
            core::Instant::from_unix_micros(unix.as_micros().min(i64::MAX as u128) as i64).ok();
        let wait = Duration::from_millis(60_000 - (unix.as_millis() % 60_000) as u64);
        cx.spawn_in(window, async move |weak, cx| {
            cx.background_executor().timer(wait).await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.schedule_clock(window, cx);
                if Date::today(&this.options.zone).ok() == Some(this.options.day) {
                    this.diagnostics.sample("minute_notify", 0, 0, 1);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn recovered(
        &mut self,
        result: std::result::Result<Payload, Failure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(failed) = self.failed_write.take() else {
            self.recovery = false;
            return;
        };
        match result {
            Ok(Payload::Entity(entity)) => {
                self.recovery = false;
                let equal = match (&failed.work, &entity) {
                    (
                        Work::SaveTask(Save::Create { data, .. } | Save::Update { data, .. }),
                        core::Entity::Task(current),
                    ) => data == &current.data,
                    (
                        Work::SaveEvent(Save::Create { data, .. } | Save::Update { data, .. }),
                        core::Entity::Event(current),
                    ) => data == &current.data,
                    (
                        Work::SaveBlock(_, Save::Create { data, .. } | Save::Update { data, .. }),
                        core::Entity::TimeBlock(current),
                    ) => data == &current.data,
                    (
                        Work::SaveCollection(CollectionSave::List(
                            Save::Create { data, .. } | Save::Update { data, .. },
                        )),
                        core::Entity::List(current),
                    ) => data == &current.data,
                    (
                        Work::SaveCollection(CollectionSave::Calendar(
                            Save::Create { data, .. } | Save::Update { data, .. },
                        )),
                        core::Entity::Calendar(current),
                    ) => data == &current.data,
                    _ => false,
                };
                if equal {
                    if let WriteOrigin::Quick { version, title } = &failed.origin {
                        self.quick_id = None;
                        if *version == self.quick_version
                            && self.quick.read(cx).value().as_ref() == title
                        {
                            self.quick_validation = None;
                            self.quick
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                    }
                    if failed
                        .origin
                        .matches_form(self.form.as_ref().map(|f| f.stamp))
                    {
                        self.discard_form();
                        self.restore_focus(window, cx);
                    }
                    self.error = false;
                    self.status = "保存先の現在値が送信内容と一致することを確認しました".into();
                } else {
                    if let Some(form) = &mut self.form
                        && failed.origin.matches_form(Some(form.stamp))
                    {
                        form.conflict = Some(entity_summary(&entity, &self.options.zone));
                    }
                    self.fail(
                        format!(
                            "現在値と下書きが異なります。自動で上書きしません。{}",
                            entity_summary(&entity, &self.options.zone)
                        ),
                        cx,
                    );
                }
            }
            Err(Failure::Core(Error::NotFound(_))) => {
                self.recovery = false;
                if matches!(failed.work, Work::Delete(_)) {
                    self.status = "対象が削除済みであることを確認しました".into();
                    if failed
                        .origin
                        .matches_form(self.form.as_ref().map(|f| f.stamp))
                    {
                        self.discard_form();
                    }
                    self.error = false;
                } else if matches!(
                    failed.work,
                    Work::SaveTask(Save::Create { .. })
                        | Work::SaveEvent(Save::Create { .. })
                        | Work::SaveBlock(_, Save::Create { .. })
                        | Work::SaveCollection(
                            CollectionSave::List(Save::Create { .. })
                                | CollectionSave::Calendar(Save::Create { .. })
                        )
                ) {
                    self.failed_write = Some(failed);
                    self.fail(
                        "作成IDは保存先に存在しません。入力を保持しています。再試行できます".into(),
                        cx,
                    );
                } else {
                    if let Some(form) = &mut self.form
                        && failed.origin.matches_form(Some(form.stamp))
                    {
                        form.conflict = Some("最新の保存先では対象が削除されています".into());
                    }
                    self.fail("対象が削除されています。下書きを保持しました".into(), cx);
                }
            }
            Err(error) => {
                self.failed_write = Some(failed);
                self.recovery = true;
                self.fail(format!("保存結果を確認できません: {error}"), cx);
            }
            _ => {
                self.failed_write = Some(failed);
                self.recovery = true;
                self.fail("保存結果の応答形式が一致しません".into(), cx);
            }
        }
        cx.notify();
    }
    pub(crate) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.recovery || self.info.is_none() {
            self.read(Work::Open, ReadContext::Open, window, cx);
            return;
        }
        if let Some(failed) = self.failed_write.take()
            && failed.can_retry(self.form.as_ref().map(|f| f.stamp))
        {
            self.write(failed.work, failed.origin, window, cx);
            return;
        }
        for (work, context, _) in self.failed_reads.0.clone() {
            if !self.read_is_current(&context) {
                self.clear_read_error(context.scope());
                continue;
            }
            let loading = match context {
                ReadContext::Tasks(..) => Some(&mut self.task_loading),
                ReadContext::Agenda(_, true, _) => Some(&mut self.all_day_loading),
                ReadContext::Agenda(_, false, _) => Some(&mut self.agenda_loading),
                _ => None,
            };
            if let Some(loading) = loading {
                if *loading {
                    continue;
                }
                *loading = true;
            }
            self.read(work, context, window, cx);
        }
        cx.notify();
    }
    pub(crate) fn apply_settings(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read(Work::Settings(settings), ReadContext::Settings, window, cx);
    }
    pub(crate) fn shift_picker_day(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(day) =
            Date::parse(&self.date_input.read(cx).value()).and_then(|day| day.add_days(delta))
        {
            self.date_input
                .update(cx, |input, cx| input.set_value(day.iso(), window, cx));
            cx.notify();
        }
    }
    pub(crate) fn apply_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing(window, cx) {
            return;
        }
        match Date::parse(&self.date_input.read(cx).value()) {
            Ok(day) => {
                self.date_validation = None;
                self.navigate(Navigation::Day(day), window, cx);
            }
            Err(error) => {
                let message = user_error(&error);
                self.date_validation = Some(message.clone());
                self.fail(message, cx);
            }
        }
    }
    pub(crate) fn shift_picker_month(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (year, month, _) = Date::parse(&self.date_input.read(cx).value())
            .unwrap_or(self.options.day)
            .ymd();
        let months = year * 12 + month as i32 - 1 + delta;
        let year = months.div_euclid(12);
        let month = months.rem_euclid(12) + 1;
        if let Ok(day) = Date::parse(&format!("{year:04}-{month:02}-01")) {
            self.date_input
                .update(cx, |input, cx| input.set_value(day.iso(), window, cx));
            cx.notify();
        }
    }
}
fn work_target(work: &Work) -> Option<core::EntityId> {
    Some(match work {
        Work::SaveTask(Save::Create { id, .. }) => core::EntityId::Task(*id),
        Work::SaveTask(Save::Update { expected, .. }) => core::EntityId::Task(expected.id),
        Work::SaveEvent(Save::Create { id, .. }) => core::EntityId::Event(*id),
        Work::SaveEvent(Save::Update { expected, .. }) => core::EntityId::Event(expected.id),
        Work::SaveBlock(_, Save::Create { id, .. }) => core::EntityId::TimeBlock(*id),
        Work::SaveBlock(_, Save::Update { expected, .. }) => core::EntityId::TimeBlock(expected.id),
        Work::SaveCollection(CollectionSave::List(Save::Create { id, .. })) => {
            core::EntityId::List(*id)
        }
        Work::SaveCollection(CollectionSave::List(Save::Update { expected, .. })) => {
            core::EntityId::List(expected.id)
        }
        Work::SaveCollection(CollectionSave::Calendar(Save::Create { id, .. })) => {
            core::EntityId::Calendar(*id)
        }
        Work::SaveCollection(CollectionSave::Calendar(Save::Update { expected, .. })) => {
            core::EntityId::Calendar(expected.id)
        }
        Work::Delete(target) => match target {
            DeleteTarget::Task(key) => core::EntityId::Task(key.id),
            DeleteTarget::Event(key) => core::EntityId::Event(key.id),
            DeleteTarget::TimeBlock { block, .. } => core::EntityId::TimeBlock(block.id),
            DeleteTarget::List(key) => core::EntityId::List(key.id),
            DeleteTarget::Calendar(key) => core::EntityId::Calendar(key.id),
        },
        _ => return None,
    })
}
fn entity_summary(entity: &core::Entity, zone: &ZoneId) -> String {
    match entity {
        core::Entity::Task(t) => format!(
            "最新: {} · {:?} · 版{}",
            t.data.title.as_str(),
            t.data.status,
            t.key.revision.get()
        ),
        core::Entity::Event(e) => format!(
            "最新: {} · 版{}",
            e.data.title.as_str(),
            e.key.revision.get()
        ),
        core::Entity::TimeBlock(b) => format!(
            "最新: {} · 版{}",
            time::span_label(&b.data.span, zone),
            b.key.revision.get()
        ),
        core::Entity::List(l) => format!("最新: {}", l.data.as_str()),
        core::Entity::Calendar(c) => format!("最新: {}", c.data.as_str()),
    }
}

#[cfg(test)]
mod task_selection_tests {
    use super::{Navigation, ReadContext};
    use crate::presentation::{
        agenda,
        task_list::{PageMode, TaskCache},
    };
    use gpui_kit::{ListAlignment, ListOffset, ListState, px};
    use ultratasks_core::*;

    fn query() -> TaskQuery {
        TaskQuery {
            day: agenda::calendar(
                Date::parse("2026-11-12").unwrap(),
                &ZoneId::parse("Asia/Tokyo").unwrap(),
            )
            .unwrap(),
            list: None,
            status: StatusFilter::Open,
            due: DueFilter::Any,
            blocks: BlockFilter::Any,
            title_contains: None,
        }
    }
    fn task(store: &mut Store, title: &str) -> Task {
        let data = TaskData {
            title: Title::new(title).unwrap(),
            list: store.info().default_list,
            status: TaskStatus::Open,
            due: None,
        };
        store
            .save_task(Save::Create {
                id: TaskId::new().unwrap(),
                data,
            })
            .unwrap()
            .value
    }
    fn page(store: &mut Store, query: TaskQuery, position: TaskPosition) -> TaskPage {
        store
            .tasks(
                query,
                TaskPageRequest {
                    size: PageSize::new(2).unwrap(),
                    position,
                },
            )
            .unwrap()
    }
    #[test]
    fn query_results_reject_hidden_selection_and_old_details_but_allow_explicit_targets() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("selection.sqlite")).unwrap();
        let a = task(&mut store, "UIレビュー");
        let b = task(&mut store, "請求書");
        let other_list = ListId::new().unwrap();
        store
            .save_collection(CollectionSave::List(Save::Create {
                id: other_list,
                data: Title::new("別のリスト").unwrap(),
            }))
            .unwrap();
        store
            .save_task(Save::Update {
                expected: a.key,
                data: TaskData {
                    list: other_list,
                    due: Some(Due::Date(Date::parse("2026-11-12").unwrap())),
                    ..a.data
                },
            })
            .unwrap();
        let initial = page(&mut store, query(), TaskPosition::First);
        let earlier = agenda::calendar(
            Date::parse("2026-11-11").unwrap(),
            &ZoneId::parse("Asia/Tokyo").unwrap(),
        )
        .unwrap();
        for (query, expected) in [
            (
                TaskQuery {
                    title_contains: Some(SearchText::new("請求書").unwrap()),
                    ..query()
                },
                vec![b.key.id],
            ),
            (
                TaskQuery {
                    list: Some(b.data.list),
                    ..query()
                },
                vec![b.key.id],
            ),
            (
                TaskQuery {
                    due: DueFilter::Undated,
                    ..query()
                },
                vec![b.key.id],
            ),
            (
                TaskQuery {
                    day: earlier,
                    due: DueFilter::ThroughSelectedDay,
                    ..query()
                },
                vec![],
            ),
        ] {
            let list = ListState::new(0, ListAlignment::Top, px(168.));
            let mut cache = TaskCache::default();
            cache.accept(initial.clone(), PageMode::Replace, &list);
            let selected = Some(a.key.id);
            let old_detail = ReadContext::ListDetail(7, a.key.id, false);
            assert!(old_detail.selection_is_current(7, selected, &cache));
            cache.invalidate();
            assert_eq!(cache.visible_selection(selected), None);
            assert!(!old_detail.selection_is_current(8, selected, &cache));
            cache.refresh(
                store
                    .task_rows(query.clone(), &[a.key.id, b.key.id])
                    .unwrap(),
                &list,
            );
            assert_eq!(cache.rows, initial.items);
            assert_eq!(cache.visible_selection(selected), None);
            cache.accept(
                page(&mut store, query, TaskPosition::First),
                PageMode::Replace,
                &list,
            );
            assert_eq!(
                cache
                    .rows
                    .iter()
                    .map(|row| row.task.key.id)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(cache.visible_selection(selected), None);
            for allocate in [false, true] {
                assert!(!Navigation::ListTask(a.key.id, allocate).is_available(&cache));
                assert!(
                    !ReadContext::ListDetail(8, a.key.id, allocate)
                        .selection_is_current(8, selected, &cache)
                );
            }
            assert!(Navigation::Task(a.key.id).is_available(&cache));
            for explicit in [
                ReadContext::Detail(8, false),
                ReadContext::Entity(8),
                ReadContext::ExistingBlock(8, TimeBlockId::new().unwrap()),
            ] {
                assert!(explicit.selection_is_current(8, None, &cache));
                assert!(!explicit.selection_is_current(9, None, &cache));
            }
            cache.accept(initial.clone(), PageMode::Replace, &list);
            assert!(!old_detail.selection_is_current(8, selected, &cache));
            let current = ReadContext::ListDetail(8, a.key.id, false);
            assert!(current.selection_is_current(8, selected, &cache));
            assert!(!current.selection_is_current(8, Some(b.key.id), &cache));
        }
    }
    #[test]
    fn appending_prepending_and_around_keep_visible_selection_and_scroll_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("pages.sqlite")).unwrap();
        let tasks = ["A", "B", "C", "D"].map(|title| task(&mut store, title));
        let first = page(&mut store, query(), TaskPosition::First);
        let second = page(
            &mut store,
            query(),
            TaskPosition::After(first.next.clone().unwrap()),
        );
        for (initial, added, mode, selected, index) in [
            (
                first.clone(),
                second.clone(),
                PageMode::Next,
                tasks[1].key.id,
                1,
            ),
            (second, first, PageMode::Previous, tasks[2].key.id, 0),
        ] {
            let list = ListState::new(0, ListAlignment::Top, px(168.));
            let mut cache = TaskCache::default();
            cache.accept(initial, PageMode::Replace, &list);
            list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item: px(5.),
            });
            cache.accept(added, mode, &list);
            assert_eq!(cache.rows.len(), 4);
            let around = page(&mut store, query(), TaskPosition::Around(selected));
            for replacement in [None, Some(around)] {
                if let Some(page) = replacement {
                    cache.accept(page, PageMode::Replace, &list);
                }
                assert_eq!(cache.visible_selection(Some(selected)), Some(selected));
                assert!(
                    ReadContext::ListDetail(3, selected, false).selection_is_current(
                        3,
                        Some(selected),
                        &cache
                    )
                );
                let (anchor, offset) = cache.anchor(&list).unwrap();
                assert_eq!(anchor, selected);
                assert_eq!(offset.offset_in_item, px(5.));
            }
            let anchor = cache.anchor(&list);
            cache.invalidate();
            assert_eq!(cache.visible_selection(Some(selected)), None);
            cache.accept(
                page(&mut store, query(), TaskPosition::Around(selected)),
                PageMode::Replace,
                &list,
            );
            assert_eq!(cache.anchor(&list), anchor);
        }
    }
    #[test]
    fn row_refresh_keeps_held_done_and_rejects_selection_when_an_edit_leaves_the_filter() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("refresh.sqlite")).unwrap();
        let mut a = task(&mut store, "UIレビュー");
        let query = TaskQuery {
            title_contains: Some(SearchText::new("UI").unwrap()),
            ..query()
        };
        let list = ListState::new(0, ListAlignment::Top, px(168.));
        let mut cache = TaskCache::default();
        cache.accept(
            page(&mut store, query.clone(), TaskPosition::First),
            PageMode::Replace,
            &list,
        );
        let selected = Some(a.key.id);
        for status in [TaskStatus::Done, TaskStatus::Open] {
            a = store
                .save_task(Save::Update {
                    expected: a.key,
                    data: TaskData { status, ..a.data },
                })
                .unwrap()
                .value;
            let rows = store.task_rows(query.clone(), &[a.key.id]).unwrap();
            assert_eq!(
                rows.matching_ids.contains(&a.key.id),
                status == TaskStatus::Open
            );
            cache.refresh(rows, &list);
            assert_eq!(cache.visible_selection(selected), selected);
            assert_eq!(
                cache.held_done.contains(&a.key.id),
                status == TaskStatus::Done
            );
        }
        store
            .save_task(Save::Update {
                expected: a.key,
                data: TaskData {
                    title: Title::new("請求書").unwrap(),
                    ..a.data
                },
            })
            .unwrap();
        cache.refresh(store.task_rows(query, &[a.key.id]).unwrap(), &list);
        assert_eq!(cache.visible_selection(selected), None);
        assert!(
            !ReadContext::ListDetail(4, a.key.id, false).selection_is_current(4, selected, &cache)
        );
        assert!(ReadContext::Detail(4, false).selection_is_current(4, None, &cache));
    }
}

#[cfg(test)]
mod form_transition_tests {
    use super::{
        FailedReads, FormStamp, PageMode, PendingWrite, ReadContext, ReadScope, Work, WriteOrigin,
    };
    use ultratasks_core::{EntityId, EventId, TaskId, TimeBlockId};
    fn pending(stamp: FormStamp) -> PendingWrite {
        PendingWrite {
            work: Work::Open,
            origin: WriteOrigin::Form { stamp },
        }
    }
    #[test]
    fn unrelated_read_success_keeps_failed_selection_and_its_retry_request() {
        let task = TaskId::new().unwrap();
        let block = TimeBlockId::new().unwrap();
        let mut failed = FailedReads::default();
        failed.record(
            Work::Detail(task),
            ReadContext::ExistingBlock(7, block),
            "詳細取得失敗".into(),
        );
        failed.record(
            Work::Open,
            ReadContext::Tasks(3, PageMode::Replace),
            "一覧取得失敗".into(),
        );
        failed.clear(ReadScope::Collections);
        failed.clear(ReadScope::AllDay);
        failed.clear(ReadScope::Rows);
        assert_eq!(failed.0.len(), 2);
        failed.clear(ReadScope::Tasks);
        assert!(failed.any());
        assert!(matches!(failed.0[0].0, Work::Detail(id) if id == task));
        assert!(matches!(failed.0[0].1, ReadContext::ExistingBlock(7, id) if id == block));
        failed.clear(ReadScope::Selection);
        assert!(!failed.any());
    }
    #[test]
    fn retry_uses_latest_failed_request_in_each_read_scope() {
        let first = EntityId::Event(EventId::new().unwrap());
        let second = EntityId::Event(EventId::new().unwrap());
        let mut failed = FailedReads::default();
        failed.record(
            Work::Get(first),
            ReadContext::Entity(1),
            "旧選択の取得失敗".into(),
        );
        failed.record(
            Work::Open,
            ReadContext::Agenda(2, false, false),
            "時間割取得失敗".into(),
        );
        failed.record(
            Work::Get(second),
            ReadContext::Entity(3),
            "新選択の取得失敗".into(),
        );
        assert_eq!(failed.0.len(), 2);
        failed.clear(ReadScope::AllDay);
        assert_eq!(failed.0.len(), 2);
        failed.clear(ReadScope::Agenda);
        assert!(matches!(failed.0[0].0, Work::Get(id) if id == second));
        assert!(matches!(failed.0[0].1, ReadContext::Entity(3)));
    }
    #[test]
    fn discarding_form_keeps_unrelated_failed_write() {
        let mut failed = Some(PendingWrite {
            work: Work::Open,
            origin: WriteOrigin::Quick {
                title: "追加中のタスク".into(),
                version: 2,
            },
        });
        assert!(!PendingWrite::abandon_form(&mut failed, false));
        assert!(failed.unwrap().can_retry(None));
    }
    #[test]
    fn failed_a_is_discarded_and_its_late_reply_cannot_close_b() {
        let a = FormStamp {
            serial: 1,
            version: 1,
        };
        let mut failed = Some(pending(a));
        assert!(failed.as_ref().unwrap().can_retry(Some(a)));
        let old_response = failed.as_ref().unwrap().origin.clone();
        assert!(PendingWrite::abandon_form(&mut failed, false));
        assert!(failed.is_none());
        let b = FormStamp {
            serial: 2,
            version: 1,
        };
        assert!(!old_response.matches_form(Some(b)));
        assert!(!pending(a).can_retry(Some(b)));
        assert!(!old_response.matches_form(None));
        assert!(pending(b).origin.matches_form(Some(b)));
    }
    #[test]
    fn edited_same_form_does_not_resend_old_values_or_accept_old_reply() {
        let saved = FormStamp {
            serial: 3,
            version: 2,
        };
        let edited = FormStamp {
            version: 3,
            ..saved
        };
        let failed = pending(saved);
        assert!(!failed.can_retry(Some(edited)));
        assert!(!failed.origin.matches_form(Some(edited)));
        assert!(pending(edited).can_retry(Some(edited)));
    }
    #[test]
    fn recovery_keeps_original_request_but_cannot_consume_another_form() {
        let a = FormStamp {
            serial: 4,
            version: 1,
        };
        let b = FormStamp {
            serial: 5,
            version: 1,
        };
        let mut failed = Some(pending(a));
        assert!(!PendingWrite::abandon_form(&mut failed, true));
        let failed = failed.unwrap();
        assert!(!failed.can_retry(Some(b)));
        assert!(!failed.origin.matches_form(Some(b)));
        assert!(failed.origin.matches_form(Some(a)));
    }
}
