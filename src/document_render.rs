use super::*;
use crate::prepared::{Block, BlockKind};

impl Adeline {
    pub(super) fn prepare_document(&mut self, index: usize, cx: &mut Context<Self>) {
        let key = (self.project, index);
        self.document_tasks.remove(&key);
        let doc = &mut self.projects[key.0].docs[key.1];
        if doc.content.len() < 32 * 1024 {
            doc.prepare();
            return;
        }
        let revision = doc.revision;
        let source = doc.content.clone();
        let task = cx
            .background_executor()
            .spawn(async move { std::sync::Arc::new(prepared::PreparedDocument::parse(&source)) });
        self.document_tasks.insert(
            key,
            cx.spawn(async move |this, cx| {
                let parsed = task.await;
                let _ = this.update(cx, |app, cx| {
                    if let Some(doc) = app
                        .projects
                        .get_mut(key.0)
                        .and_then(|p| p.docs.get_mut(key.1))
                        && doc.publish_prepared(revision, parsed)
                    {
                        if app.project == key.0 {
                            app.files_region.update(cx, |_, cx| cx.notify());
                            app.files_home_region.update(cx, |_, cx| cx.notify());
                            if app.document == Some(key.1) {
                                app.document_region
                                    .update(cx, |view, cx| view.sync(app, cx));
                            }
                        }
                        cx.notify();
                    }
                });
            }),
        );
    }
    pub(super) fn document_block(
        &self,
        block: &Block,
        focus: Option<&FocusHandle>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let j = block.line;
        if !self.raw {
            match &block.kind {
                BlockKind::Image(asset) => {
                    return img(ImageSource::Resource(Resource::Embedded((*asset).into())))
                        .w_full()
                        .h(px(456.))
                        .flex_shrink_0()
                        .rounded_lg()
                        .object_fit(ObjectFit::Contain)
                        .mb_3()
                        .into_any_element();
                }
                BlockKind::Check { checked, label } => {
                    return self
                        .button(("check-line", j), "", Action::CheckLine(j), cx)
                        .when_some(focus, |button, focus| button.track_focus(focus))
                        .h_auto()
                        .py_1()
                        .px_0()
                        .child(
                            div()
                                .size(px(14.))
                                .flex_shrink_0()
                                .rounded(px(2.))
                                .border_1()
                                .border_color(rgb(if *checked {
                                    theme::primary()
                                } else {
                                    theme::muted_foreground()
                                }))
                                .when(*checked, |d| {
                                    d.bg(rgb(theme::primary())).child(
                                        icon("check")
                                            .size(px(12.))
                                            .text_color(rgb(theme::primary_foreground())),
                                    )
                                }),
                        )
                        .child(
                            text(
                                SharedString::from(label.clone()),
                                14.,
                                if *checked {
                                    theme::muted_foreground()
                                } else {
                                    theme::foreground()
                                },
                            )
                            .when(*checked, |d| d.line_through()),
                        )
                        .into_any_element();
                }
                BlockKind::Table(cells) => {
                    return row()
                        .w_full()
                        .border_b_1()
                        .border_color(rgb(theme::border()))
                        .children(cells.iter().map(|cell| {
                            text(SharedString::from(cell.clone()), 14., theme::foreground())
                                .flex_1()
                                .min_w_0()
                                .whitespace_normal()
                                .overflow_hidden()
                                .p_2()
                        }))
                        .into_any_element();
                }
                BlockKind::Separator => return div().into_any_element(),
                BlockKind::Text { .. } => {}
            }
        }
        let (display, heading) = if self.raw {
            (block.raw.clone(), 0)
        } else if let BlockKind::Text { display, heading } = &block.kind {
            (display.clone(), *heading)
        } else {
            (block.raw.clone(), 0)
        };
        div()
            .w_full()
            .min_w_0()
            .id(("paragraph", j))
            .cursor(CursorStyle::IBeam)
            .on_click(cx.listener(move |s, _, w, cx| s.act(Action::EditLine(j), w, cx)))
            .child(
                text(
                    SharedString::from(display),
                    if heading == 3 {
                        16.
                    } else if heading > 0 {
                        20.
                    } else {
                        14.
                    },
                    theme::foreground(),
                )
                .w_full()
                .min_w_0()
                .line_height(px(26.))
                .when(heading > 0, |d| d.mt_3().font_weight(FontWeight::SEMIBOLD))
                .when(self.raw, |d| d.font_family("monospace")),
            )
            .into_any_element()
    }
}
