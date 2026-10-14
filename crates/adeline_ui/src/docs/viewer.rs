//! The Markdown Viewer: the Kit's text view with front matter as a table,
//! GitHub alerts, task checkboxes that edit the file, mermaid diagrams,
//! project images read through the engine, and remote images shown as links
//! unless the user allows loading them.
//!
//! Renderers run outside the app's entities, so clicks reach `Adeline` as
//! actions carrying the byte offsets of the doc's source they act on.

// Rust guideline compliant 2026-10-09

use super::{NoteRange, OpenLink, ToggleTask};
use gpui_kit::base::text::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
    MarkdownPlugin, TextView, TextViewState, markdown_ast as mdast,
};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants as _},
    text::FrontmatterPlugin,
};
use gpui_kit::{prelude::*, *};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::{Arc, LazyLock, Mutex},
};

/// Project images by project path: loaded, or `None` when they can't be read.
#[derive(Clone, Default)]
pub(crate) struct Images {
    loaded: Arc<Mutex<HashMap<String, Option<Arc<Image>>>>>,
    wanted: Arc<Mutex<HashSet<String>>>,
}

impl Images {
    pub(crate) fn wanted(&self) -> Vec<String> {
        self.wanted
            .lock()
            .map(|mut wanted| wanted.drain().collect())
            .unwrap_or_default()
    }

    pub(crate) fn put(&self, path: String, image: Option<Arc<Image>>) {
        if let Ok(mut loaded) = self.loaded.lock() {
            loaded.insert(path, image);
        }
    }

    // An image not asked for yet is wanted for the next frame.
    fn get(&self, path: &str) -> Option<Arc<Image>> {
        let found = self.loaded.lock().ok()?.get(path).cloned();
        if found.is_none()
            && let Ok(mut wanted) = self.wanted.lock()
        {
            wanted.insert(path.to_owned());
        }
        found.flatten()
    }
}

static PLACEHOLDER: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#.to_vec(),
    ))
});

fn remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

const ALERTS: [(&str, &str, &str); 5] = [
    ("NOTE", "Note", "info.svg"),
    ("TIP", "Tip", "lightbulb.svg"),
    ("IMPORTANT", "Important", "warning-circle.svg"),
    ("WARNING", "Warning", "warning.svg"),
    ("CAUTION", "Caution", "warning-octagon.svg"),
];

#[derive(Clone)]
struct Alert {
    kind: usize,
    body: SharedString,
}

#[derive(Clone)]
struct Task {
    checked: bool,
    // Where the `[ ]` or `[x]` sits in the doc's source.
    mark: usize,
    body: SharedString,
}

#[derive(Clone)]
struct Diagram {
    source: SharedString,
    range: Range<usize>,
}

fn alert(node: &mdast::Node, parse: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
    let mdast::Node::Blockquote(_) = node else {
        return None;
    };
    let source = parse.node_source(node)?;
    let lines: Vec<&str> = source
        .lines()
        .map(|line| {
            let line = line.trim_start();
            line.strip_prefix("> ")
                .or_else(|| line.strip_prefix('>'))
                .unwrap_or(line)
        })
        .collect();
    let marker = lines.first()?.trim();
    let kind = ALERTS
        .iter()
        .position(|(name, _, _)| marker.eq_ignore_ascii_case(&format!("[!{name}]")))?;
    let body = lines[1..].join("\n");
    Some(
        MarkdownNode::new(
            "alert",
            Alert {
                kind,
                body: body.clone().into(),
            },
        )
        .text(body),
    )
}

fn task(node: &mdast::Node, parse: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
    let mdast::Node::ListItem(item) = node else {
        return None;
    };
    let checked = item.checked?;
    let start = item.position.as_ref()?.start.offset;
    let source = parse.node_source(node)?;
    let open = source.find('[')?;
    let close = source[open..].find(']')? + open;
    let first = source[close + 1..].trim_start_matches([' ', '\t']);
    let indent = close + 2;
    let body: Vec<&str> = std::iter::once(first.lines().next().unwrap_or_default())
        .chain(source.lines().skip(1).map(|line| {
            let trimmed = line.len() - line.trim_start().len();
            &line[trimmed.min(indent)..]
        }))
        .collect();
    let body = body.join("\n");
    Some(
        MarkdownNode::new(
            "task",
            Task {
                checked,
                mark: parse.offset() + start + open,
                body: body.clone().into(),
            },
        )
        .text(body),
    )
}

fn diagram(node: &mdast::Node, parse: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
    let mdast::Node::Code(code) = node else {
        return None;
    };
    if code.lang.as_deref() != Some("mermaid") {
        return None;
    }
    let position = code.position.as_ref()?;
    let range = parse.offset() + position.start.offset..parse.offset() + position.end.offset;
    Some(
        MarkdownNode::new(
            "mermaid",
            Diagram {
                source: code.value.clone().into(),
                range,
            },
        )
        .text(code.value.clone()),
    )
}

fn link_handler(url: &SharedString, _: &ClickEvent, window: &mut Window, cx: &mut App) {
    window.dispatch_action(Box::new(OpenLink { url: url.clone() }), cx);
}

fn nested(id: impl Into<ElementId>, body: &SharedString, cx: &App) -> TextView {
    TextView::markdown(id, body.clone())
        .selectable(crate::panes::selectable_in(crate::panes::DOCS_MAIN, cx))
        .on_link_click(link_handler)
        .w_full()
        .min_w_0()
}

fn render_alert(node: &MarkdownNode, _: &mut Window, cx: &mut App) -> AnyElement {
    let Some(alert) = node.data::<Alert>() else {
        return div().into_any_element();
    };
    let (_, title, icon) = ALERTS[alert.kind];
    let theme = cx.theme();
    let color = match alert.kind {
        0 => theme.info,
        1 => theme.success,
        2 => theme.primary,
        3 => theme.warning,
        _ => theme.danger,
    };
    div()
        .w_full()
        .my_2()
        .pl_3()
        .py_1()
        .border_l_2()
        .border_color(color)
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .text_color(color)
                .font_weight(FontWeight::MEDIUM)
                .child(Icon::default().path(icon).small())
                .child(title),
        )
        .child(nested(
            ("alert", node.source_range().map_or(0, |r| r.start)),
            &alert.body,
            cx,
        ))
        .into_any_element()
}

fn render_task(node: &MarkdownNode, _: &mut Window, cx: &mut App) -> AnyElement {
    let Some(task) = node.data::<Task>().cloned() else {
        return div().into_any_element();
    };
    let label = if task.checked {
        "Mark as not done"
    } else {
        "Mark as done"
    };
    div()
        .w_full()
        .flex()
        .items_start()
        .gap_2()
        .child(
            Button::new(("task", task.mark))
                .ghost()
                .xsmall()
                .icon(Icon::default().path(if task.checked {
                    "check-square.svg"
                } else {
                    "square.svg"
                }))
                .text_color(cx.theme().foreground)
                .accessibility_label(label)
                .tooltip(label)
                .on_click(move |_, window, cx| {
                    window.dispatch_action(Box::new(ToggleTask { offset: task.mark }), cx);
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(nested(("task-body", task.mark), &task.body, cx)),
        )
        .into_any_element()
}

fn render_diagram(node: &MarkdownNode, _: &mut Window, cx: &mut App) -> AnyElement {
    let Some(diagram) = node.data::<Diagram>().cloned() else {
        return div().into_any_element();
    };
    let theme = cx.theme();
    let dark = theme.is_dark();
    // ponytail: renders on every frame the diagram is laid out; cache by source if it shows in profiles.
    let rendered = super::mermaid::render(&diagram.source, dark);
    let range = diagram.range.clone();
    let note = Button::new(("note-diagram", range.start))
        .ghost()
        .xsmall()
        .label("Note on this diagram")
        .on_click(move |_, window, cx| {
            window.dispatch_action(
                Box::new(NoteRange {
                    start: range.start,
                    end: range.end,
                }),
                cx,
            );
        });
    let body = match rendered {
        Ok(image) => img(image)
            .max_w_full()
            .object_fit(ObjectFit::ScaleDown)
            .into_any_element(),
        Err(reason) => div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(format!("Can't render: {reason}")),
            )
            .child(
                div()
                    .p_2()
                    .rounded(theme.radius)
                    .bg(theme.muted)
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(diagram.source.clone()),
            )
            .into_any_element(),
    };
    div()
        .id(("mermaid", diagram.range.start))
        .w_full()
        .my_2()
        .flex()
        .flex_col()
        .gap_1()
        .role(Role::Figure)
        .aria_label("Mermaid diagram")
        .child(body)
        .child(div().flex().justify_end().child(note))
        .into_any_element()
}

struct RemoteImage;

#[derive(Clone)]
struct Link {
    url: SharedString,
    alt: SharedString,
}

impl MarkdownPlugin for RemoteImage {
    fn name(&self) -> &'static str {
        "remote-image"
    }

    fn parse(&self, node: &mdast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Image(image) = node else {
            return None;
        };
        if !remote(&image.url) {
            return None;
        }
        let alt = if image.alt.is_empty() {
            image.url.clone()
        } else {
            image.alt.clone()
        };
        Some(
            MarkdownNode::new(
                "remote-image",
                Link {
                    url: image.url.clone().into(),
                    alt: alt.clone().into(),
                },
            )
            .text(alt),
        )
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        _: &InlineRenderContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let link = node.data::<Link>()?.clone();
        let label = format!("Image: {}", link.alt);
        Some(InlineElement::new(
            div()
                .id(SharedString::from(format!("remote-image:{}", link.url)))
                .role(Role::Link)
                .aria_label(label.clone())
                .text_color(cx.theme().link)
                .underline()
                .cursor_pointer()
                .child(label)
                .on_click(move |_, window, cx| {
                    window.dispatch_action(
                        Box::new(OpenLink {
                            url: link.url.clone(),
                        }),
                        cx,
                    );
                }),
        ))
    }
}

/// The Viewer for the Markdown doc at `path`, whose text `state` holds.
pub(crate) fn markdown(
    state: &Entity<TextViewState>,
    path: &str,
    images: &Images,
    remote_images: bool,
    selectable: bool,
) -> TextView {
    let mut extensions = MarkdownExtensions::default()
        .frontmatter()
        .plugin(FrontmatterPlugin::new());
    if !remote_images {
        extensions = extensions.plugin(RemoteImage);
    }
    let extensions = extensions
        .block_parser(alert)
        .block_parser(task)
        .block_parser(diagram)
        .block_renderer("alert", render_alert)
        .block_renderer("task", render_task)
        .block_renderer("mermaid", render_diagram);
    let images = images.clone();
    let from = path.to_owned();
    TextView::new(state)
        .markdown_extensions(extensions)
        .scrollable(true)
        .selectable(selectable)
        .on_link_click(link_handler)
        .image_source(move |uri| {
            let url = uri.as_ref();
            if remote(url) {
                return ImageSource::Resource(Resource::Uri(uri.clone()));
            }
            let found = adeline_docs::join(&from, url.split('#').next().unwrap_or(url))
                .and_then(|path| images.get(&path));
            ImageSource::Image(found.unwrap_or_else(|| Arc::clone(&PLACEHOLDER)))
        })
}
