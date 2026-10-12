//! General › Appearance: theme, fonts and font sizes.
use super::*;

fn font_label(which: usize) -> &'static str {
    if which == 0 {
        "Interface font"
    } else {
        "Code font"
    }
}

impl SettingsWindow {
    pub(super) fn save_font_size(&mut self, which: usize, size: u16, cx: &mut Context<Self>) {
        if !(config::MIN_FONT_SIZE..=config::MAX_FONT_SIZE).contains(&size) {
            self.font_size_errors[which] = Some(font_size_error());
        } else if size == selected_font_size(which) {
            self.font_size_errors[which] = None;
        } else {
            match config::update(|settings| {
                if which == 0 {
                    settings.general.appearance.font_size = size;
                } else {
                    settings.general.appearance.code_font_size = size;
                }
            }) {
                Ok(()) => {
                    self.font_size_errors[which] = None;
                    theme::apply(cx);
                }
                Err(message) => self.font_size_errors[which] = Some(message),
            }
        }
        cx.notify();
    }
    pub(super) fn choose_font(&mut self, which: usize, family: &str, cx: &mut Context<Self>) {
        match config::update(|settings| {
            if which == 0 {
                settings.general.appearance.interface_font = family.into();
            } else {
                settings.general.appearance.code_font = family.into();
            }
        }) {
            Ok(()) => {
                self.theme_status = None;
                theme::apply(cx);
            }
            Err(message) => self.theme_status = Some(message),
        }
        cx.notify();
    }
    fn refresh_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match theme::discover() {
            Ok((choices, errors)) => {
                let saved = config::current().general.appearance.theme;
                self.theme_picker.update(cx, |picker, cx| {
                    picker.set_items(SearchableVec::new(choices), window, cx);
                    picker.set_selected_value(&saved, window, cx);
                });
                self.theme_status = (!errors.is_empty())
                    .then(|| errors.join("\n"))
                    .or_else(theme::load_error);
            }
            Err(message) => self.theme_status = Some(message),
        }
        cx.notify();
    }

    fn refresh_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        fonts::refresh(cx);
        for (which, picker) in self.font_pickers.iter().enumerate() {
            let mut names = fonts::families();
            let saved = saved_font(which);
            if !names.iter().any(|name| name.eq_ignore_ascii_case(&saved)) && !saved.is_empty() {
                names.insert(0, saved.clone());
            }
            picker.update(cx, |picker, cx| {
                picker.set_items(SearchableVec::new(names), window, cx);
                picker.set_selected_value(&saved, window, cx);
            });
        }
        theme::apply(cx);
        cx.notify();
    }

    pub(super) fn appearance_settings(&self, cx: &Context<Self>) -> Div {
        let mut page = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                Form::new().child(
                    Field::new()
                        .label("Theme")
                        .description("Themes are loaded from the Adeline themes folder.")
                        .child(
                            Select::new(&self.theme_picker)
                                .accessibility_label("Theme")
                                .search_placeholder("Search themes"),
                        ),
                ),
            )
            .child(
                Button::new("refresh-themes")
                    .small()
                    .ghost()
                    .label("Refresh themes")
                    .on_click(cx.listener(|this, _, window, cx| this.refresh_themes(window, cx))),
            );
        if let Some(status) = &self.theme_status {
            page = page.child(error(status.clone(), cx));
        }
        for which in 0..2 {
            let fallback = if which == 0 {
                fonts::DEFAULT
            } else {
                fonts::CODE_DEFAULT
            };
            let saved = saved_font(which);
            let resolved = if which == 0 {
                config::font()
            } else {
                config::code_font()
            };
            page = page.child(
                Form::new().child(
                    Field::new()
                        .label(font_label(which))
                        .description(format!(
                            "{fallback} is used if the selected font is unavailable."
                        ))
                        .child(
                            Select::new(&self.font_pickers[which])
                                .accessibility_label(font_label(which))
                                .search_placeholder("Search installed fonts"),
                        ),
                ),
            );
            if saved != resolved {
                page = page.child(error(
                    format!("{saved} is unavailable. Using {resolved}."),
                    cx,
                ));
            }
            let size_label = if which == 0 {
                "Interface font size"
            } else {
                "Code font size"
            };
            page = page.child(
                Form::new().child(
                    Field::new()
                        .label(size_label)
                        .description(format!(
                            "A whole number from {} to {} pixels. Changes save immediately.",
                            config::MIN_FONT_SIZE,
                            config::MAX_FONT_SIZE
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new(format!("decrease-font-size-{which}"))
                                        .label("Smaller")
                                        .accessibility_label(format!("Decrease {size_label}"))
                                        .disabled(
                                            selected_font_size(which) <= config::MIN_FONT_SIZE,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.step_font(which, -1, window, cx);
                                        })),
                                )
                                .child(div().w_16().child(
                                    Input::new(&self.font_sizes[which]).aria_label(size_label),
                                ))
                                .child(
                                    Button::new(format!("increase-font-size-{which}"))
                                        .label("Larger")
                                        .accessibility_label(format!("Increase {size_label}"))
                                        .disabled(
                                            selected_font_size(which) >= config::MAX_FONT_SIZE,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.step_font(which, 1, window, cx);
                                        })),
                                ),
                        ),
                ),
            );
            if let Some(message) = &self.font_size_errors[which] {
                page = page.child(error(message.clone(), cx));
            }
        }
        page.child(
            Button::new("refresh-fonts")
                .small()
                .ghost()
                .label("Refresh installed fonts")
                .on_click(cx.listener(|this, _, window, cx| this.refresh_fonts(window, cx))),
        )
    }
    fn step_font(&mut self, which: usize, step: i16, window: &mut Window, cx: &mut Context<Self>) {
        let next = selected_font_size(which)
            .saturating_add_signed(step)
            .clamp(config::MIN_FONT_SIZE, config::MAX_FONT_SIZE);
        self.save_font_size(which, next, cx);
        self.font_sizes[which].update(cx, |input, cx| {
            input.set_value(selected_font_size(which).to_string(), window, cx);
        });
    }
}
