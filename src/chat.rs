//! Independently invalidated chat regions. The shell remains the shared
//! data/action coordinator; list state and input invalidation live in these views.
use super::*;
use crate::Action;
use gpui_kit::component::{
    ActiveTheme as _,
    input::{InputEvent, TextareaState},
    scroll::Scrollbar,
};
use std::sync::Arc;

pub(super) struct ChatList {
    owner: WeakEntity<Adeline>,
    state: ListState,
    typography: config::Typography,
    visible: Arc<[usize]>,
    keys: Vec<Arc<str>>,
    records: prepared::SearchCatalog,
    generation: prepared::Generation,
    pending: Option<Task<()>>,
    searching: bool,
    search_bytes: usize,
    project: usize,
    counts: [usize; 4],
    selected: Option<usize>,
    query: String,
    filter: usize,
    completed: bool,
}

impl ChatList {
    pub fn new(owner: WeakEntity<Adeline>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(180.));
        Self {
            owner,
            state,
            typography: config::typography(),
            visible: Arc::from([]),
            keys: Vec::new(),
            records: Default::default(),
            generation: Default::default(),
            pending: None,
            searching: false,
            search_bytes: 0,
            project: usize::MAX,
            counts: [0; 4],
            selected: None,
            query: String::new(),
            filter: 0,
            completed: true,
        }
    }

    pub fn sync(&mut self, app: &Adeline, query: &str, cx: &mut Context<Self>) {
        let threads = &app.workspace().threads;
        let records: prepared::SearchCatalog = threads
            .iter()
            .map(|t| prepared::SearchRecord {
                id: Arc::from(t.id.as_str()),
                text: t.search_text.clone(),
                completed: !t.matches_status(0, false),
                blocked: t.matches_status(1, true),
                working: t.matches_status(2, true),
                unread: t.unread(),
            })
            .collect();
        if self.project != app.project
            || self.records.len() != records.len()
            || self
                .records
                .iter()
                .zip(records.iter())
                .any(|(a, b)| a.id != b.id)
        {
            self.visible = Arc::from([]);
            self.keys.clear();
            self.state.reset(0);
        }
        self.project = app.project;
        self.counts = [records.len(), 0, 0, 0];
        for r in records.iter() {
            self.counts[1] += usize::from(r.blocked);
            self.counts[2] += usize::from(r.working);
            self.counts[3] += usize::from(r.unread);
        }
        self.search_bytes = records.iter().map(|r| r.text.len()).sum();
        self.records = records;
        self.selected = app.selected;
        self.search(query.to_owned(), app.filter, app.show_completed, false, cx);
    }

    /// A model mutation identifies one stable thread; selection alone is view state.
    fn update_thread(&mut self, app: &Adeline, cx: &mut Context<Self>) {
        if self.project != app.project || self.records.len() != app.workspace().threads.len() {
            self.sync(app, &self.query.clone(), cx);
            return;
        }
        if let Some(index) = app.selected {
            let thread = &app.workspace().threads[index];
            if self.records[index].id.as_ref() != thread.id {
                self.sync(app, &self.query.clone(), cx);
                return;
            }
            let old = &self.records[index];
            let record = prepared::SearchRecord {
                id: old.id.clone(),
                text: thread.search_text.clone(),
                completed: !thread.matches_status(0, false),
                blocked: thread.status == "blocked",
                working: thread.status == "working",
                unread: thread.unread(),
            };
            let same_text = old.text.same_snapshot(&record.text);
            if same_text
                && old.completed == record.completed
                && old.blocked == record.blocked
                && old.working == record.working
                && old.unread == record.unread
            {
                self.selected = app.selected;
                cx.notify();
                return;
            }
            let known_match = if !record.matches_filter(self.filter, self.completed) {
                Some(false)
            } else if same_text
                && old.matches_filter(self.filter, self.completed)
                && !self.searching
            {
                Some(self.visible.binary_search(&index).is_ok())
            } else {
                None
            };
            self.search_bytes = self.search_bytes - old.text.len() + record.text.len();
            for (slot, (before, after)) in self.counts[1..].iter_mut().zip([
                (old.blocked, record.blocked),
                (old.working, record.working),
                (old.unread, record.unread),
            ]) {
                *slot = *slot - usize::from(before) + usize::from(after);
            }
            self.records.replace(index, record);
            if let Ok(row) = self.visible.binary_search(&index) {
                invalidate_range(&self.state, row..row + 1);
            }
            // A pending query must restart on the new immutable snapshot. Explicit
            // actions skip the typing delay. Large text checks stay on the worker.
            if self.searching
                || (known_match.is_none()
                    && !self.query.is_empty()
                    && self.records[index].text.len() >= 64 * 1024)
            {
                self.search(self.query.clone(), self.filter, self.completed, false, cx);
            } else {
                let matches = known_match.unwrap_or_else(|| {
                    self.records[index].matches(&self.query, self.filter, self.completed)
                });
                match self.visible.binary_search(&index) {
                    Ok(row) if !matches => {
                        let mut visible = self.visible.to_vec();
                        visible.remove(row);
                        self.publish(visible, cx);
                    }
                    Err(row) if matches => {
                        let mut visible = self.visible.to_vec();
                        visible.insert(row, index);
                        self.publish(visible, cx);
                    }
                    _ => {}
                }
            }
        }
        self.selected = app.selected;
        cx.notify();
    }

    pub fn search(
        &mut self,
        query: String,
        filter: usize,
        completed: bool,
        debounce: bool,
        cx: &mut Context<Self>,
    ) {
        self.pending.take();
        self.query.clone_from(&query);
        self.filter = filter;
        self.completed = completed;
        let ticket = self.generation.next();
        let records = self.records.clone();
        if query.is_empty() || (records.len() < 256 && self.search_bytes < 64 * 1024) {
            self.publish(
                prepared::search_records(records.iter(), &query, filter, completed),
                cx,
            );
        } else {
            self.searching = true;
            let executor = cx.background_executor().clone();
            self.pending = Some(cx.spawn(async move |this, cx| {
                if debounce {
                    executor.timer(std::time::Duration::from_millis(30)).await;
                }
                let rows = executor
                    .spawn(async move {
                        prepared::search_records(records.iter(), &query, filter, completed)
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.generation.accepts(ticket) {
                        this.publish(rows, cx);
                    }
                });
            }));
            cx.notify();
        }
    }

    fn publish(&mut self, visible: Vec<usize>, cx: &mut Context<Self>) {
        let keys: Vec<_> = visible
            .iter()
            .map(|&i| self.records[i].id.clone())
            .collect();
        let (old, new) = prepared::changed_range(&self.keys, &keys);
        if !old.is_empty() || !new.is_empty() {
            self.state.splice(old, new.len());
        }
        self.visible = visible.into();
        self.keys = keys;
        self.searching = false;
        cx.notify();
    }
}

impl Render for ChatList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = config::typography();
        if self.typography != typography {
            self.typography = typography;
            let top = self.state.logical_scroll_top();
            let count = self.state.item_count();
            self.state.splice(0..count, count);
            self.state.scroll_to(top);
        }
        ui_metrics::record(ui_metrics::Region::Sidebar);
        let owner = self.owner.clone();
        let visible = self.visible.clone();
        let navigation = visible.clone();
        let navigation_owner = owner.clone();
        let navigation_state = self.state.clone();
        let selected = self.selected;
        let list_focus = window
            .use_keyed_state("chat-list-keyboard", cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let rows = if visible.is_empty() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if self.searching {
                    "Searching…"
                } else {
                    "No chats found"
                })
                .p_5()
                .flex_1()
                .into_any_element()
        } else {
            div()
                .id("chat-list-viewport")
                .relative()
                .flex_1()
                .min_h_0()
                .track_focus(&list_focus)
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    let current = selected.and_then(|ix| navigation.binary_search(&ix).ok());
                    let target = match event.keystroke.key.as_str() {
                        "up" => current.unwrap_or(0).saturating_sub(1),
                        "down" => current.map_or(0, |ix| (ix + 1).min(navigation.len() - 1)),
                        "home" => 0,
                        "end" => navigation.len() - 1,
                        _ => return,
                    };
                    navigation_state.scroll_to_reveal_item(target);
                    list_focus.focus(window, cx);
                    window.refresh();
                    if selected != Some(navigation[target]) {
                        let _ = navigation_owner.update(cx, |app, cx| {
                            app.act(Action::Chat(navigation[target]), window, cx);
                        });
                    }
                    cx.stop_propagation();
                })
                .child(
                    list(self.state.clone(), move |row, _, cx| {
                        ui_metrics::record(ui_metrics::Region::ChatRow);
                        owner
                            .update(cx, |app, cx| app.chat_card(visible[row], cx))
                            .unwrap_or_else(|_| div().into_any_element())
                    })
                    .size_full(),
                )
                .child(Scrollbar::vertical(&self.state))
                .into_any_element()
        };
        self.owner
            .update(cx, |app, cx| {
                app.chat_sidebar(rows, self.counts, cx).into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(super) struct Transcript {
    owner: WeakEntity<Adeline>,
    state: ListState,
    typography: config::Typography,
    thread: Option<(usize, String)>,
    selected: Option<usize>,
    messages: usize,
    footer_focus: FocusHandle,
}

impl Transcript {
    pub fn new(owner: WeakEntity<Adeline>, cx: &mut Context<Self>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(250.));
        Self {
            owner,
            state,
            typography: config::typography(),
            thread: None,
            selected: None,
            messages: 0,
            footer_focus: cx.focus_handle(),
        }
    }

    pub fn sync(&mut self, app: &Adeline, scroll_to_end: bool, cx: &mut Context<Self>) {
        let thread = app.selected.map(|i| &app.workspace().threads[i]);
        let key = thread.map(|t| (app.project, t.id.clone()));
        let messages = thread.map_or(0, |t| t.messages.len());
        let footer = thread.is_some_and(|t| {
            !app.demo_mode
                || t.status == "blocked"
                || app
                    .workspace()
                    .decisions
                    .iter()
                    .any(|d| d.thread_id == t.id)
        });
        let count = messages + usize::from(footer);
        let previous_top = self.state.logical_scroll_top();
        let changed_thread = key != self.thread;
        if changed_thread {
            self.state.reset(0);
        }
        // Streaming can change the final message without appending another row.
        let start = if changed_thread {
            0
        } else if app.demo_mode {
            self.messages.min(messages)
        } else {
            self.messages.saturating_sub(1).min(messages)
        };
        if start != count || start != self.state.item_count() {
            self.state.splice_focusable(
                start..self.state.item_count(),
                (start..count).map(|i| (i == messages).then(|| self.footer_focus.clone())),
            );
        }
        if scroll_to_end {
            self.state.scroll_to(ListOffset {
                item_ix: count,
                offset_in_item: px(0.),
            });
        } else if !changed_thread {
            self.state.scroll_to(previous_top);
        }
        self.thread = key;
        self.selected = app.selected;
        self.messages = messages;
        cx.notify();
    }
}

impl Render for Transcript {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = config::typography();
        if self.typography != typography {
            self.typography = typography;
            let top = self.state.logical_scroll_top();
            let count = self.state.item_count();
            self.state.splice(0..count, count);
            self.state.scroll_to(top);
        }
        ui_metrics::record(ui_metrics::Region::Transcript);
        let Some(selected) = self.selected else {
            return self
                .owner
                .update(cx, |app, _cx| app.welcome().size_full().into_any_element())
                .unwrap_or_else(|_| div().into_any_element());
        };
        let owner = self.owner.clone();
        let messages = self.messages;
        let footer_focus = self.footer_focus.clone();
        let rows = list(self.state.clone(), move |row, _, cx| {
            ui_metrics::record(ui_metrics::Region::MessageRow);
            owner
                .update(cx, |app, cx| {
                    if row < messages {
                        app.message_row(selected, row, cx)
                    } else {
                        col()
                            .w_full()
                            .track_focus(&footer_focus)
                            .px_5()
                            .child(if app.demo_mode {
                                app.decision_row(selected, cx)
                            } else {
                                app.runtime_footer(selected, cx)
                            })
                            .into_any_element()
                    }
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full()
        .py_2();
        div()
            .relative()
            .size_full()
            .min_h_0()
            .child(rows)
            .child(Scrollbar::vertical(&self.state))
            .into_any_element()
    }
}

fn invalidate_range(state: &ListState, range: std::ops::Range<usize>) {
    let top = state.logical_scroll_top();
    state.splice(range.clone(), range.len());
    state.scroll_to(top);
}

pub(super) struct Composer {
    owner: WeakEntity<Adeline>,
    _content_subscription: Subscription,
}

impl Composer {
    pub fn new(
        owner: WeakEntity<Adeline>,
        input: &Entity<TextareaState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            owner,
            _content_subscription: subscription,
        }
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Composer);
        self.owner
            .update(cx, |app, cx| app.composer_view(cx).into_any_element())
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(super) struct Header(pub WeakEntity<Adeline>);
impl Render for Header {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Header);
        self.0
            .update(cx, |app, cx| app.header(cx).w_full().into_any_element())
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Adeline {
    pub(super) fn search_sidebar(&self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.chat_list.update(cx, |list, cx| {
            // Programmatic input resets can emit the same InputEvent::Change.
            if list.query != query
                || list.filter != self.filter
                || list.completed != self.show_completed
            {
                list.search(query, self.filter, self.show_completed, true, cx);
            }
        });
    }
    pub(super) fn sync_sidebar(&self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.chat_list
            .update(cx, |list, cx| list.sync(self, &query, cx));
    }

    pub(super) fn sync_regions(&self, action: &Action, cx: &mut Context<Self>) {
        use crate::Action::*;
        if matches!(
            action,
            Section(_)
                | Project(_)
                | NewChat
                | ShowCompleted
                | HideToolCalls
                | LeftPanel(_)
                | RightPanel(_)
                | ToggleLeftPanel
                | ToggleSidePanel
        ) {
            self.control_pane.update(cx, |_, cx| cx.notify());
        }
        // Data mutations notify their dependents explicitly. Opening overlays,
        // changing focus and changing unrelated sections do not invalidate them.
        match action {
            Project(_) => self.sync_sidebar(cx),
            Section(crate::Section::Chats) | NewChat | Filter(_) | ShowCompleted => {
                let query = self.query(cx);
                self.chat_list.update(cx, |list, cx| {
                    list.selected = self.selected;
                    list.search(query, self.filter, self.show_completed, false, cx);
                });
            }
            Chat(_) | Complete | Send | Decision(_) => {
                self.chat_list
                    .update(cx, |list, cx| list.update_thread(self, cx));
            }
            _ => {}
        }
        if matches!(
            action,
            Project(_) | Chat(_) | NewChat | Complete | Send | Decision(_)
        ) {
            let end = matches!(action, Send);
            self.transcript
                .update(cx, |view, cx| view.sync(self, end, cx));
        }
        if matches!(
            action,
            Project(_) | Chat(_) | NewChat | Send | Complete | Agent(_) | Machine(_)
        ) {
            self.composer_region.update(cx, |_, cx| cx.notify());
        }
        if matches!(
            action,
            Project(_)
                | Machine(_)
                | Section(_)
                | Chat(_)
                | NewChat
                | Complete
                | Send
                | Decision(_)
                | SaveSettings
        ) {
            self.header_region.update(cx, |_, cx| cx.notify());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::invalidate_range;
    use gpui_kit::{ListAlignment, ListOffset, ListState, px};

    #[test]
    fn remeasuring_variable_rows_preserves_the_scroll_anchor() {
        let list = ListState::new(1000, ListAlignment::Top, px(180.));
        list.scroll_to(ListOffset {
            item_ix: 600,
            offset_in_item: px(29.),
        });
        invalidate_range(&list, 600..601);
        assert_eq!(list.logical_scroll_top().item_ix, 600);
        assert_eq!(list.logical_scroll_top().offset_in_item, px(29.));
        assert_eq!(list.item_count(), 1000);
    }
}
