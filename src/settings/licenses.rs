//! The Licenses pages for bundled third-party assets.
use super::*;

impl SettingsWindow {
    pub(super) fn licenses_settings(&self, cx: &Context<Self>) -> Vec<Div> {
        [
            (
                "Phosphor Icons",
                "Primary interface icons · MIT · Copyright (c) 2020-2024 Phosphor Icons",
                include_str!("../../assets/PHOSPHOR-LICENSE.txt"),
            ),
            (
                "GPUI",
                "Native interface toolkit · Apache-2.0 · Zed Industries",
                include_str!("../../assets/GPUI-LICENSE.txt"),
            ),
            (
                "Chivo & Chivo Mono",
                "Bundled interface and code fonts · SIL Open Font License 1.1",
                fonts::LICENSE,
            ),
        ]
        .into_iter()
        .enumerate()
        .filter(|(index, _)| self.subgroup.is_none_or(|child| child == *index))
        .map(|(_, (name, description, license))| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_lg().child(name))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
                .child(div().p_4().bg(cx.theme().muted).child(license))
        })
        .collect()
    }
}
