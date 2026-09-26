//! Cached content regions and variable-height virtual documents/service output.
use super::*;
use crate::prepared::{BlockKind, PreparedDocument};
use std::sync::Arc;

pub(super) struct Files(pub WeakEntity<Adeline>);
// Keep the lightweight wrappers uncached: GPUI 0.2.2 forces nested cache
// refreshes while rebuilding a cached parent. Cache heavy sibling regions.
pub(super) struct DocsHome(pub WeakEntity<Adeline>);
impl Render for DocsHome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::DocsHome);
        self.0
            .update(cx, |app, cx| app.files_home_view(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}
pub(super) struct ServiceSidebar(pub WeakEntity<Adeline>);
impl Render for ServiceSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::ServiceSidebar);
        self.0
            .update(cx, |app, cx| app.service_sidebar_view(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}
impl Render for Files {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Docs);
        self.0
            .update(cx, |app, cx| app.files(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(super) struct Services(pub WeakEntity<Adeline>);
impl Render for Services {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui_metrics::record(ui_metrics::Region::Services);
        self.0
            .update(cx, |app, cx| app.services_view(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

pub(super) struct DocumentView {
    owner: WeakEntity<Adeline>,
    state: ListState,
    typography: config::Typography,
    scrollbar: scrollbar::Scrollbar,
    key: Option<(usize, usize)>,
    parsed: Arc<PreparedDocument>,
    rows: Arc<[usize]>,
    focus: Arc<[Option<FocusHandle>]>,
    loading: bool,
    revision: u64,
    raw: bool,
}
impl DocumentView {
    pub fn new(owner: WeakEntity<Adeline>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(250.));
        Self {
            owner,
            scrollbar: scrollbar::Scrollbar::list(state.clone()),
            state,
            typography: config::typography(),
            key: None,
            parsed: Default::default(),
            rows: Arc::from([]),
            focus: Arc::from([]),
            loading: false,
            revision: 0,
            raw: false,
        }
    }
    pub fn sync(&mut self, app: &Adeline, cx: &mut Context<Self>) {
        let key = app.document.map(|i| (app.project, i));
        let Some((project, index)) = key else {
            self.key = None;
            self.state.reset(0);
            cx.notify();
            return;
        };
        let doc = &app.projects[project].docs[index];
        let changed = self.key != key;
        let loading = doc.revision != doc.prepared_revision;
        if !changed
            && !loading
            && self.raw == app.raw
            && let Some((base, block)) = doc.prepared_edit
            && base == self.revision
            && let Ok(row) = self.rows.binary_search(&block)
            && self.focus[row].is_some()
                == matches!(doc.prepared.blocks[block].kind, BlockKind::Check { .. })
        {
            let top = self.state.logical_scroll_top();
            self.parsed = doc.prepared.clone();
            self.revision = doc.revision;
            self.loading = false;
            self.state
                .splice_focusable(row + 1..row + 2, [self.focus[row].clone()]);
            self.state.scroll_to(top);
            cx.notify();
            return;
        }
        if !changed && loading {
            // Keep this document's last valid snapshot while its replacement is
            // prepared. Action handlers reject edits against stale source lines.
            self.loading = true;
            let top = self.state.logical_scroll_top();
            if self.state.item_count() > 0 {
                self.state.splice(0..1, 1);
                self.state.scroll_to(top);
            }
            cx.notify();
            return;
        }
        if !changed
            && !loading
            && !self.loading
            && self.revision == doc.revision
            && self.raw == app.raw
        {
            // Titles can change without changing parsed body content.
            let top = self.state.logical_scroll_top();
            if self.state.item_count() > 0 {
                self.state.splice(0..1, 1);
                self.state.scroll_to(top);
            }
            cx.notify();
            return;
        }
        let mut top = self.state.logical_scroll_top();
        let old_parsed = self.parsed.clone();
        let old_rows = self.rows.clone();
        let old_focus = self.focus.clone();
        let mode_changed = self.raw != app.raw;
        let source_line = top
            .item_ix
            .checked_sub(1)
            .and_then(|r| self.rows.get(r))
            .map(|&r| self.parsed.blocks[r].line);
        self.key = key;
        self.loading = loading;
        self.revision = doc.revision;
        self.raw = app.raw;
        self.parsed = doc.prepared.clone();
        self.rows = if app.raw {
            (0..self.parsed.blocks.len()).collect::<Vec<_>>().into()
        } else {
            self.parsed.rich_rows.as_slice().into()
        };
        let before: Vec<_> = old_rows
            .iter()
            .map(|&r| old_parsed.blocks[r].raw.clone())
            .collect();
        let after: Vec<_> = self
            .rows
            .iter()
            .map(|&r| self.parsed.blocks[r].raw.clone())
            .collect();
        let (old_range, new_range) = prepared::changed_range(&before, &after);
        self.focus = self
            .rows
            .iter()
            .enumerate()
            .map(|(row, &r)| {
                matches!(self.parsed.blocks[r].kind, BlockKind::Check { .. }).then(|| {
                    if !changed {
                        let old_row = if row < new_range.start {
                            Some(row)
                        } else if row >= new_range.end {
                            Some(row - new_range.end + old_range.end)
                        } else if old_range.len() == 1 && new_range.len() == 1 {
                            Some(old_range.start)
                        } else {
                            None
                        };
                        if let Some(focus) = old_row
                            .and_then(|i| old_focus.get(i))
                            .and_then(Option::as_ref)
                        {
                            return focus.clone();
                        }
                    }
                    cx.focus_handle().tab_stop(true)
                })
            })
            .collect();
        if changed {
            self.state.reset(0);
        }
        if changed || mode_changed {
            self.state.splice_focusable(
                0..self.state.item_count(),
                std::iter::once(None).chain(self.focus.iter().cloned()),
            );
        } else {
            let (old, new) = (old_range, new_range);
            if !old.is_empty() || !new.is_empty() {
                let same_count = old.len() == new.len();
                self.state
                    .splice_focusable(old.start + 1..old.end + 1, self.focus[new].iter().cloned());
                if !same_count {
                    top = self.state.logical_scroll_top();
                }
            }
        }
        if !changed {
            self.state.splice(0..1, 1);
            if mode_changed && let Some(line) = source_line {
                top.item_ix = self
                    .rows
                    .iter()
                    .position(|&r| self.parsed.blocks[r].line >= line)
                    .map_or(self.rows.len(), |r| r + 1);
            }
            self.state.scroll_to(top);
        }
        cx.notify();
    }
}
impl Render for DocumentView {
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let typography = config::typography();
        if self.typography != typography {
            self.typography = typography;
            let top = self.state.logical_scroll_top();
            let count = self.state.item_count();
            self.state.splice(0..count, count);
            self.state.scroll_to(top);
        }
        ui_metrics::record(ui_metrics::Region::Document);
        if self.loading && self.parsed.blocks.is_empty() {
            return text("Preparing document…", 14., theme::muted_foreground())
                .p_6()
                .into_any_element();
        }
        let Some((_, index)) = self.key else {
            return div().into_any_element();
        };
        let owner = self.owner.clone();
        let parsed = self.parsed.clone();
        let rows = self.rows.clone();
        let focus = self.focus.clone();
        let loading = self.loading;
        let content = list(self.state.clone(), move |row_index, window, cx| {
            ui_metrics::record(ui_metrics::Region::DocumentRow);
            owner
                .update(cx, |app, cx| {
                    // GPUI 0.2.2 measures virtual rows with MinContent height.
                    // A nested percentage width plus max-width can measure text
                    // before applying the cap, underestimating wrapped heights.
                    let width = (f32::from(window.viewport_size().width)
                        - 18.
                        - app.left_panel_width()
                        - if app.side_panel_is_open() {
                            app.right_panel_width
                        } else {
                            0.
                        })
                    .clamp(1., 800.);
                    let mut content = col().w(px(width)).min_w_0().flex_shrink_0().px_5().pb_3();
                    if row_index == 0 {
                        content = content.pt_6().child(
                            div()
                                .id("doc-title")
                                .cursor(CursorStyle::IBeam)
                                .on_click(
                                    cx.listener(|s, _, w, cx| s.act(Action::EditTitle, w, cx)),
                                )
                                .child(
                                    text(
                                        app.workspace().docs[index].title.clone(),
                                        30.,
                                        theme::foreground(),
                                    )
                                    .mb_5()
                                    .when(loading, |d| {
                                        d.child(text(
                                            "Updating preview…",
                                            12.,
                                            theme::muted_foreground(),
                                        ))
                                    }),
                                ),
                        );
                    } else {
                        content = content.child(app.document_block(
                            &parsed.blocks[rows[row_index - 1]],
                            focus[row_index - 1].as_ref(),
                            cx,
                        ));
                    }
                    col()
                        .w_full()
                        .items_center()
                        .child(content)
                        .into_any_element()
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full()
        .pb_6();
        div()
            .relative()
            .size_full()
            .min_h_0()
            .child(content)
            .child(self.scrollbar.element())
            .into_any_element()
    }
}

pub(super) struct LogView {
    state: ListState,
    typography: config::Typography,
    scrollbar: scrollbar::Scrollbar,
    lines: Arc<[Arc<str>]>,
    key: Option<usize>,
    stopped: bool,
    wrap: bool,
    max_width: f32,
}
impl LogView {
    pub fn new(_: WeakEntity<Adeline>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(180.));
        Self {
            scrollbar: scrollbar::Scrollbar::list(state.clone()),
            state,
            typography: config::typography(),
            lines: Arc::from([]),
            key: None,
            stopped: false,
            wrap: true,
            max_width: 0.,
        }
    }
    pub fn sync(&mut self, app: &Adeline, cx: &mut Context<Self>) {
        let changed = self.key != app.service;
        let old_lines = self.lines.clone();
        let wrap_changed = self.wrap != app.wrap;
        let was_stopped = self.stopped;
        self.key = app.service;
        self.lines = app
            .service
            .map_or_else(|| Arc::from([]), |i| app.services[i].lines.clone());
        self.max_width = app
            .service
            .map_or(0., |i| app.services[i].max_line_chars as f32 * 8. + 48.);
        self.wrap = app.wrap;
        self.stopped = app.stopped;
        let count = self.lines.len() + usize::from(self.stopped);
        let top = self.state.logical_scroll_top();
        if changed {
            self.state.reset(0);
        }
        if changed || wrap_changed {
            self.state.splice(0..self.state.item_count(), count);
        } else if !Arc::ptr_eq(&old_lines, &self.lines) {
            let (old, new) = prepared::changed_range(&old_lines, &self.lines);
            self.state.splice(old, new.len());
            if was_stopped != self.stopped {
                self.state.splice(
                    self.lines.len()..self.state.item_count(),
                    usize::from(self.stopped),
                );
            }
        } else if was_stopped != self.stopped {
            self.state.splice(
                self.lines.len()..self.state.item_count(),
                usize::from(self.stopped),
            );
        }
        if app.follow {
            self.state.scroll_to(ListOffset {
                item_ix: count,
                offset_in_item: px(0.),
            });
        } else if !changed {
            self.state.scroll_to(top);
        }
        cx.notify();
    }
}
impl Render for LogView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let typography = config::typography();
        if self.typography != typography {
            self.typography = typography;
            let top = self.state.logical_scroll_top();
            let count = self.state.item_count();
            self.state.splice(0..count, count);
            self.state.scroll_to(top);
        }
        ui_metrics::record(ui_metrics::Region::Log);
        let lines = self.lines.clone();
        let wrap = self.wrap;
        let rows = list(self.state.clone(), move |i, _, _| {
            ui_metrics::record(ui_metrics::Region::LogRow);
            let line = lines
                .get(i)
                .cloned()
                .unwrap_or_else(|| Arc::from("Service stopped in this local demo."));
            text(SharedString::from(line), 12., theme::foreground())
                .w_full()
                .min_h(px(23.))
                .px_6()
                .font_family(config::code_font())
                .text_size(config::code_text_pixels(14.))
                .line_height(config::code_text_pixels(23.))
                .when(!wrap, |d| d.whitespace_nowrap())
                .into_any_element()
        })
        .w_full()
        .h_full()
        .py_6()
        .when(!wrap, |d| d.min_w(config::code_text_pixels(self.max_width)));
        div()
            .relative()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .id("service-log-horizontal")
                    .size_full()
                    .overflow_x_scroll()
                    .bg(rgb(theme::background()))
                    .child(rows),
            )
            .child(self.scrollbar.element())
    }
}

impl Adeline {
    pub(super) fn sync_content_regions(&mut self, action: &Action, cx: &mut Context<Self>) {
        use crate::Action::*;
        if matches!(
            action,
            Project(_)
                | Section(_)
                | DocsHome
                | ToggleLeftPanel
                | Document(_)
                | Raw
                | Archive
                | NewDoc
                | PinDoc
                | SaveLine
                | Format(_)
                | CheckLine(_)
                | SaveTitle
                | ArchiveDoc
                | SaveSettings
                | Filter(_)
        ) {
            self.files_region.update(cx, |_, cx| cx.notify());
            self.files_home_region.update(cx, |_, cx| cx.notify());
        }
        if matches!(
            action,
            ToggleLeftPanel
                | ToggleSidePanel
                | Project(_)
                | Document(_)
                | NewDoc
                | DocsHome
                | Archive
                | ArchiveDoc
                | Raw
                | SaveLine
                | Format(_)
                | CheckLine(_)
                | SaveTitle
        ) {
            self.document_region
                .update(cx, |view, cx| view.sync(self, cx));
        }
        if matches!(
            action,
            Project(_)
                | Section(_)
                | Service(_)
                | NewService
                | StopService
                | Wrap
                | Follow
                | Filter(_)
        ) {
            self.services_region.update(cx, |_, cx| cx.notify());
            self.service_sidebar_region.update(cx, |_, cx| cx.notify());
        }
        if matches!(
            action,
            Project(_) | Service(_) | NewService | StopService | Wrap | Follow
        ) {
            self.log_region.update(cx, |view, cx| view.sync(self, cx));
        }
    }
}
