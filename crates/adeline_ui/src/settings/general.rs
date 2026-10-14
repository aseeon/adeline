//! General › Features and Keymap, and Modes › Chats and Docs.
use super::*;

fn keymap_rows() -> Vec<String> {
    let keys = config::current().general.keymap;
    vec![
        keys.open_settings.join(" / "),
        keys.new_chat.join(" / "),
        keys.focus_search.join(" / "),
        keys.send_message.join(" / "),
        keys.close_dialog_or_popup.join(" / "),
        keys.next_control.join(" / "),
        keys.previous_control.join(" / "),
        "Enter / Space".into(),
    ]
}

impl SettingsWindow {
    pub(super) fn features_settings(
        &self,
        query: &str,
        searching: bool,
        cx: &Context<Self>,
    ) -> Div {
        let mut page = div().w_full().flex().flex_col().gap_3();
        if searching || self.subgroup.is_none() {
            page = page.child(div().text_lg().child("Features"));
        }
        page = page.child(div().text_sm().text_color(cx.theme().muted_foreground)
            .child("Choose which features are available in the main view. Chats is always enabled."));
        let features = config::current().general.features;
        let mut modes = Vec::new();
        // Modes still in the works show only in the demo.
        for (section, name) in MODES
            .into_iter()
            .skip(1)
            .filter(|(section, _)| self.demo || *section == Section::Docs)
        {
            if matches_query(
                query,
                &["General", "Features", name, "Enable mode in the main view"],
            ) {
                modes.push(self.setting_row(
                    format!("feature-{name}"),
                    name,
                    "",
                    features.enabled(section),
                    Action::ToggleMode(section),
                    cx,
                ));
            }
        }
        if let Some(card) = card(modes, cx) {
            page = page.child(caps("Modes", cx).mt_2()).child(card);
        }
        page
    }

    pub(super) fn keymap_settings(&self, query: &str, searching: bool, cx: &Context<Self>) -> Div {
        let mut page = div().w_full().flex().flex_col().gap_3();
        if searching || self.subgroup.is_none() {
            page = page.child(div().text_lg().child("Keymap"));
        }
        page = page.child(div().text_sm().text_color(cx.theme().muted_foreground)
            .child("Edit shortcuts in settings.yml, then restart Adeline. Enter and Space activate focused controls."));
        let mut keys = Vec::new();
        for (label, value) in KEYMAP.into_iter().zip(keymap_rows()) {
            if !matches_query(query, &["General", "Keymap", label, &value]) {
                continue;
            }
            keys.push(
                div()
                    .flex()
                    .justify_between()
                    .gap_4()
                    .px_4()
                    .py_3()
                    .child(label)
                    .child(
                        div()
                            .text_sm()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().muted_foreground)
                            .child(value),
                    ),
            );
        }
        page.children(card(keys, cx))
    }

    pub(super) fn chats_settings(
        &self,
        app: &Adeline,
        query: &str,
        searching: bool,
        cx: &Context<Self>,
    ) -> Option<Div> {
        let options: Vec<_> = app
            .mode_options(Section::Chats)
            .into_iter()
            .enumerate()
            .filter(|(_, option)| matches_query(query, &["Modes", "Chats", option.0, option.1]))
            .collect();
        let thinking_matches = matches_query(
            query,
            &["Modes", "Chats", THINKING_LABEL, THINKING_DESCRIPTION],
        );
        let silence_matches = matches_query(
            query,
            &["Modes", "Chats", SILENCE_LABEL, SILENCE_DESCRIPTION, "hang"],
        );
        if options.is_empty() && !thinking_matches && !silence_matches {
            return None;
        }
        let mut page = div().w_full().flex().flex_col().gap_3();
        if searching || self.subgroup.is_none() {
            page = page.child(div().text_lg().child("Chats"));
        }
        let mut rows: Vec<Div> = options
            .into_iter()
            .map(|(index, (label, description, checked, action))| {
                self.setting_row(
                    format!("chats-setting-{index}"),
                    label,
                    description,
                    checked,
                    action,
                    cx,
                )
            })
            .collect();
        if thinking_matches {
            rows.push(self.thinking_animation_row(cx));
        }
        if silence_matches {
            rows.push(self.silence_row(cx));
        }
        Some(page.children(card(rows, cx)))
    }

    pub(super) fn docs_settings(
        &self,
        app: &Adeline,
        query: &str,
        searching: bool,
        cx: &Context<Self>,
    ) -> Option<Div> {
        let rows: Vec<Div> = app
            .mode_options(Section::Docs)
            .into_iter()
            .enumerate()
            .filter(|(_, option)| matches_query(query, &["Modes", "Docs", option.0, option.1]))
            .map(|(index, (label, description, checked, action))| {
                self.setting_row(
                    format!("docs-setting-{index}"),
                    label,
                    description,
                    checked,
                    action,
                    cx,
                )
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        let mut page = div().w_full().flex().flex_col().gap_3();
        if searching || self.subgroup.is_none() {
            page = page.child(div().text_lg().child("Docs"));
        }
        Some(page.children(card(rows, cx)))
    }

    fn silence_row(&self, cx: &Context<Self>) -> Div {
        field_row(
            Field::new()
                .label(SILENCE_LABEL)
                .description(SILENCE_DESCRIPTION)
                .child(
                    div()
                        .w_24()
                        .child(Input::new(&self.silence_minutes).aria_label(SILENCE_LABEL)),
                ),
            self.silence_error.clone(),
            cx,
        )
    }

    fn thinking_animation_row(&self, cx: &Context<Self>) -> Div {
        field_row(
            Field::new()
                .label(THINKING_LABEL)
                .description(THINKING_DESCRIPTION)
                .child(
                    div().w_64().child(
                        Select::new(&self.thinking_picker).accessibility_label(THINKING_LABEL),
                    ),
                ),
            self.thinking_error.clone(),
            cx,
        )
    }
}
