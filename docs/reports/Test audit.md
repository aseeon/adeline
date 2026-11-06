# Test audit

Date: 2026-11-06, at commit f27f989. Covers all 204 tests in the workspace.

Each test was read with the production code it runs and checked against five questions: does it test Adeline's own logic rather than a library or UI kit, is it a duplicate, does it have a real use case, does it test as little code as it can, and would a plausible regression go uncaught without it.

Result: 145 high, 49 mid, 10 low.

## Bugs found while reading

- `tokens` (`crates/adeline_ui/src/chat.rs:367`) prints "1000k" for 999,500 to 999,999 tokens. It rounds after it decides between k and M. `token_counts_round_to_short_units` doesn't reach that range.
- `duration` (`crates/adeline_ui/src/activity.rs:111`) prints "60.0s" for 59,950 to 59,999 ms instead of "1m 00s".
- `excerpt` in activity.rs always adds "…" to CRLF text, because `lines()` drops the `\r` and the length check then counts the text as cut.

## Assertions that can't fail

- `storage/tests.rs:822` `a_torn_last_line_is_cut_off_and_the_conversation_stays_writable`: `!text.contains("assis\"")` can't fail, because the torn fragment ends in `assis` with no quote. The reload check later in the test still catches the regression. Asserting that every line parses as JSON would make this check work.
- `adeline/tests/engine.rs:444` `a_reconnect_gets_only_the_deltas_it_missed_in_order`: `text < turn_end` holds when `text` is `None`, so it never proves the missed text arrived.
- `adeline_core/tests/protocol.rs` `every_message_kind_has_a_sample`: `variants()` parses serde's error text. If that wording changes it returns an empty list and the test passes. It needs `assert!(!names.is_empty())`.
- `adeline_core/src/config.rs` `new_settings_never_seed_excluded_mode_preferences`: the update sets `docs = true`, which is already the default, so the update changes nothing.
- `adeline/tests/ui.rs` `upgrade_and_browse`: `sites[0].contains("site")` repeats the filter that built `sites`.
- `adeline/tests/ui.rs` `open_folder_name_taken`: counts notifications without checking which one appeared.
- `adeline_engine/src/acp/tests.rs` `temporary_failure_obeys_retry_limit_and_preserves_original_until_work`: the "preserves original" half is vacuous, because `fail()` never touches the prompt. The `worked` → CONTINUE_TURN path is never run.
- `adeline_engine/src/acp/tests.rs` `shutdown_during_setup_never_starts_configuration_or_prompt`: still passes with the `closing` guard in `session_ready` removed.
- `adeline_ui/src/theme.rs` `theme_names_cannot_leave_the_themes_folder`: the first check rejects all five inputs, so the later checks never run, and with no accepted name it would also pass if `theme_path` rejected everything.

## Low value

| Test | Why |
|---|---|
| `adeline_core/src/config.rs` `new_settings_never_seed_excluded_mode_preferences` | Tests a serde `skip_serializing_if` attribute. The final assert is vacuous. A regression only writes harmless null keys. |
| `adeline_core/src/data.rs` `prepared_search_matches_original_full_text_semantics_and_edits` | Tautological: it rebuilds `prepare_search`'s own formula and compares the two. `appended_messages_keep_search_semantics_and_unread_counts` already covers the useful part. |
| `adeline_core/src/data.rs` `fixture_keeps_chat_activity_and_decisions_with_their_projects` | Lints the demo `workspace.json` asset, not code. |
| `adeline_core/src/harness.rs` `guidance_names_the_agent_before_the_instructions` | A one-line `format!` helper. |
| `adeline_engine/src/acp/tests.rs` `agent_without_model_or_effort_options_keeps_its_defaults` | An empty selection is skipped before the options are looked up, so the name describes nothing it tests. Every engine.rs turn runs this path. |
| `adeline_engine/src/acp/tests.rs` `shutdown_during_setup_never_starts_configuration_or_prompt` | Passes with the guard it targets removed. Its Stopped half duplicates `cancel_discards_pending_retry_and_permission`. |
| `adeline/tests/ui.rs` `launch` | Every live UI test already starts the engine. Only the Windows GUI-subsystem check is unique. Move that into another test. |
| `adeline_ui/tests/ui.rs` `the_window_title_centers_on_the_main_area` | Layout and styling only. A regression is cosmetic. |
| `adeline_ui/src/theme.rs` `bundled_themes_load_with_all_tokens` | `bundled_themes_populate_fresh_config_and_preserve_existing_files` already validates every bundled theme. The token count is tautological, since `validate` requires it. Only the unique-names check is new. |
| `adeline_ui/src/docs/mermaid.rs` `renders_each_diagram_kind_to_png` | Tests mermaid-rs-renderer and resvg. Adeline's own limits (`MAX_SOURCE_BYTES`, `MAX_PIXELS`, empty diagram) have no test. Its only use is catching a breaking dependency upgrade. |

## Mid value

| Test | Why |
|---|---|
| **adeline_core** | |
| `agents.rs` `registry_agents_store_identity_and_defaults_but_no_command` | Pins serde field names in the YAML. The useful part is one `validate` branch. The round trip repeats `persists_complete_definitions_and_refreshes_external_changes`. |
| `agents.rs` `adding_copies_the_harness_icon_and_renaming_keeps_it` | Can't catch the rename regression it names: the avatar is the harness icon, so re-copying it on rename looks the same. Use a custom avatar. |
| `config.rs` `legacy_font_settings_load_and_each_size_is_bounded_on_its_own` | The legacy `glass` load is real. The rest tests `clamp` and a serde round trip. |
| `config.rs` `legacy_features_use_new_defaults_and_explicit_choices_persist` | Mostly asserts `Features::default` values and mirrors the `enabled`/`toggle` arms. |
| `data.rs` `completed_and_archived_conversations_show_only_when_asked_for` | The filter half duplicates `prepared.rs` `sections_put_live_chats_first_then_newest_by_period_then_finished`. Only the status-to-flag mapping is new. |
| `data.rs` `demo_chats_fill_every_section_whenever_the_demo_runs` | The `shift_to` check is logic. The section counts lint demo content. |
| `files.rs` `replace_creates_then_replaces_and_leaves_no_temp_files` | Every save test already runs `replace`. Only the no-leftover-temp check is new. |
| `log.rs` `events_reach_the_log_file_with_their_fields` | Pins tracing-subscriber's field format. Needs nextest's one process per test. |
| `conversation.rs` `the_current_step_is_the_running_one_else_the_next_pending` | A four-line helper, though the order could regress. |
| `conversation.rs` `statuses_keep_their_saved_names` | Mostly serde attributes, but they keep saved chats loading. The `MessageRole` assert is pure serde. |
| `platform.rs` `owner_only_pipe_accepts_this_user` | Never tests the rejection its name implies. Every engine.rs test already connects through the pipe. |
| `profiles.rs` `profiles_are_found_by_harness_or_reported_identity` | `find` is real. The rest pins profile constants. |
| **adeline_docs** | |
| `tree.rs` `ten_thousand_docs_filter_quickly` | Mostly times the nucleo library. Catches only an O(n²) slip, and the 2 s limit can flake on a busy CI runner. |
| **adeline_engine** | |
| `acp/tests.rs` `changing_retries_affects_the_running_turn` | Tests a one-line assignment. The engine forwarding is not tested. |
| `acp/tests.rs` `unavailable_session_needs_consent_but_missing_auth_needs_login` | The consent half duplicates engine.rs `a_lost_session_waits_for_consent_and_the_replacement_carries_the_history`. Only the auth half is unique. |
| `acp/tests.rs` `claude_gets_instructions_in_session_meta_with_the_name_sentence_first` | Mirrors the three-arm match in `session_meta` and pins `harness::guidance` wording. |
| `acp/tests.rs` `session_state_between_turns_is_reported` | About six field mappings. Only "outside a turn" and the `_meta.steering` parse matter. |
| `acp/tests.rs` `an_agent_on_another_protocol_version_gets_a_version_error` | One `if`. Cheap, low risk. |
| `acp/tests.rs` `driver_reports_an_agent_that_exits_at_once` | Mostly duplicates engine.rs `an_agent_that_exits_mid_turn_is_restarted_and_without_retries_its_stderr_is_shown`. Unique only in exiting before the handshake. |
| `storage/tests.rs` `project_rename_preserves_history_and_directory_snapshot` | About ten checks. The launch-change refusal and the foreign-file delete block duplicate other storage tests. Split it. |
| `storage/tests.rs` `legacy_opened_time_is_read_and_survives_a_rename` | A legacy path that loses value over time. |
| `storage/tests.rs` `transcript_replays_messages_tool_updates_errors_and_raw_traffic` | About twelve checks, several of them `adeline_core` `Thread` logic. The missing-workdir check at the end is unrelated. Split it. |
| `engine/docs.rs` `a_doc_too_large_to_read_is_refused_without_reading_it` | One size comparison. "Without reading it" is not checked. |
| **adeline (integration)** | |
| `tests/engine.rs` `text_for_another_session_never_reaches_the_conversation` | A one-line sessionId guard run through the whole binary. It belongs in acp/tests.rs. |
| `tests/engine.rs` `docs_an_agent_writes_get_one_card_each_and_watching_clients_see_disk_changes` | About ten checks. Its CRLF and bad-name checks duplicate `adeline_docs` and `engine/docs.rs` tests. The card wiring and watch deltas are unique: split those out. |
| `tests/ui.rs` `quit_idle` | Covers the not-busy branch. A regression only asks a needless question. |
| `tests/ui.rs` `copy_code_block` | Needs no engine. It runs live only because the demo workspace has no code block. Add one and move the test to the demo suite. |
| `tests/ui.rs` `open_folder_name_taken` | Passes on any notification and never checks that no second project was created. |
| **adeline_ui** | |
| `tests/ui.rs` `traffic_needs_verbose` | Only checks that traffic is hidden. It would pass if Ctrl+Shift+L broke. |
| `tests/ui.rs` `demo_machines` | Hide/show and the disconnected label duplicate live `drop_and_reconnect`. Only the Settings General/Engine check is unique. |
| `tests/ui.rs` `starting_and_switching_tabs_open_last_active_chat` | Checks that some chat opens, not the last active one. |
| `tests/ui.rs` `the_composer_glides_between_chats_and_the_right_island` | The mid-animation check is styling. The end positions are the useful part. |
| `tests/ui.rs` `demo_fork` | Demo-only branch. Checks the banner, not the copied messages. |
| `tests/ui.rs` `page_down_and_up_scroll_the_viewer` | Compares every label in the window, so any label change passes. |
| `tests/ui.rs` `docs_diagrams_render_in_the_background` | Same waits as pixels.rs `mermaid_diagram_renders`, but the only version that runs on Windows. Passes if one of two diagrams fails. |
| `tests/ui.rs` `ligatures_start_off_and_switch_on` | "Starts off" asserts a default. Never checks the fonts lose `calt`/`liga`. |
| `src/theme.rs` `discovers_added_themes_and_reports_invalid_files` | Its seed no-overwrite check tests `adeline_core`. Discovery overlaps the seeding test. |
| `src/theme.rs` `missing_theme_falls_back_to_embedded_islands` | Calls `State::fallback` directly, so the `init()` wiring isn't covered. |
| `src/theme.rs` `theme_yaml_round_trips_and_rejects_missing_or_bad_colors` | A grab bag. The round trip and `deny_unknown_fields` test serde. |
| `src/fonts.rs` `default_legacy_empty_and_unknown_fonts_use_the_bundle` | A four-line helper. |
| `src/chat/list.rs` `offsets_round_trip_through_items` | Simple `partition_point`. Skips the clamping cases. |
| `src/chat/list.rs` `remeasuring_variable_rows_preserves_the_scroll_anchor` | Mostly exercises GPUI's `ListState` around a three-line helper. |
| `src/docs/mermaid.rs` `invalid_sources_give_a_reason` | Pins the library's error format. |
| `src/activity.rs` `durations_read_at_a_glance` | Copies each branch. Misses the 59.95 s bug. An unrelated `excerpt` assert is tacked on. |
| `src/titlebar.rs` `portrait_raster_matches_display_pixels` | Keeps two size lists in step. Failure is a blank icon. |
| `src/mcp_ui.rs` `name_value_lines_parse_and_reject_bare_names` | Small parser. Misses a value that contains the separator. |
| `src/interaction.rs` `closing_tabs_preserves_an_open_selection` | Misses the wraparound branch, which ui.rs `close_tab` covers. |
| `src/chat/transcript.rs` `a_paragraph_quoted_on_every_line_unquotes` | An eight-line helper. |
| `src/chat/composer.rs` `token_counts_round_to_short_units` | Misses the "1000k" bug. Tests a function in chat.rs. |

## High value

High-value tests that should be split because each checks many separate behaviors: `adeline_ui/tests/ui.rs` `demo_conversation` (about 8 features), `docs_demo` (about 10 flows) and `right_island_resizes_in_every_mode`. Also `adeline/tests/ui.rs` `upgrade_and_browse`, whose browse half needs no download but only runs in the ignored step.

**adeline_core** (src unit tests)
- agents.rs: `a_saved_permission_mode_loads_and_is_dropped_on_save`, `old_format_files_are_silently_ignored_and_left_unchanged`, `persists_complete_definitions_and_refreshes_external_changes`, `normalizes_names_but_never_overwrites_collisions`, `validates_fields_and_reports_bad_neighbors_without_hiding_valid_agents`, `external_edit_or_invalid_or_deleted_file_conflicts_until_overwrite`, `renames_and_deletes_without_damaging_existing_destinations`, `demo_store_never_reads_or_writes_real_definitions`
- card.rs: `routes_name_their_mode_and_keep_the_path_opaque`
- config.rs: `legacy_values_survive_unrelated_updates_without_validation_or_recreation`, `malformed_active_settings_are_not_overwritten_while_legacy_is_ignored`, `seeding_preserves_existing_file`
- data.rs: `appended_messages_keep_search_semantics_and_unread_counts`, `a_turn_summary_counts_distinct_files_and_every_call`, `timing_counts_turns_to_their_last_answer_and_steady_streams_only`
- harness.rs: `registry_entries_become_local_launch_commands`, `guessed_executables_must_come_from_their_package`, `package_metadata_names_the_executable`, `registry_env_version_and_profiles_shape_the_launch`, `versions_compare_by_number_not_text`
- log.rs: `a_full_log_rotates_and_keeps_three_old_copies`
- conversation.rs: `base64_round_trips_every_padding`, `selections_keep_one_value_per_option`
- machines.rs: `destinations_split_off_a_numeric_port`, `an_unparsable_file_blocks_saving`, `a_failed_save_leaves_the_machines_unchanged`
- platform.rs: `only_groups_still_ours_are_stopped`, `piped_child_echoes_stdin_and_reports_stderr`, `stop_children_kills_agent_groups`
- prepared.rs: `row_changes_keep_the_common_prefix_and_suffix`, `sections_put_live_chats_first_then_newest_by_period_then_finished`, `rail_lists_every_chat_in_list_order_without_the_open_one`, `counts_describe_what_each_agent_would_show`
- protocol.rs: `engine_and_client_reach_the_same_transcript`
- recency.rs: `parses_stored_milliseconds_and_demo_timestamps_to_the_same_instant`, `periods_follow_calendar_days`, `labels_shorten_with_recency`
- remote.rs: `probes_name_release_platforms`, `ssh_failures_sort_by_whether_retrying_helps`

**adeline_core/tests/protocol.rs**: `every_message_reads_back_unchanged`, `every_message_kind_has_a_sample`

**adeline_docs**
- format.rs: all 11 tests
- files.rs: `listing_and_single_checks_agree_on_which_files_are_docs`, `only_paths_inside_the_project_resolve`, `links_join_relative_to_the_linking_doc`, `a_dangling_link_does_not_resolve`, `names_that_would_break_or_overwrite_something_are_refused`
- tree.rs: `folders_come_first_and_each_level_is_alphabetical`, `search_finds_abbreviations_and_opens_their_folders`
- text.rs: `saving_keeps_line_endings_and_the_final_newline_as_they_were`, `unreadable_text_opens_read_only_with_a_reason`

**adeline_engine**
- acp/tests.rs: `forks_use_the_native_fork_and_fall_back_to_the_text_copy`, `effort_offered_only_after_the_model_is_set_is_still_applied`, `switching_while_idle_sets_the_option_and_while_stopped_waits_for_setup`, `session_modes_hide_the_profiles_plan_modes_and_switch_by_set_mode`, `cancel_discards_pending_retry_and_permission`, `denied_permission_settles_without_retry_even_when_retries_remain`, `error_looking_reply_is_reported_without_retry`, `temporary_failure_obeys_retry_limit_and_preserves_original_until_work`, `unavailable_saved_model_fails_before_prompt`, `a_profile_turn_end_signal_ends_the_turn_and_its_late_answer_is_ignored`, `failure_classification_avoids_retrying_auth_and_configuration`, `a_steer_still_waiting_when_the_agent_goes_is_reported_undelivered`, `files_are_embedded_or_linked_and_images_need_support`, `a_noisy_agent_with_string_ids_and_unknown_updates_keeps_working`, `dropping_the_driver_stops_its_agent`, `a_broken_pipe_counts_as_the_agent_closing`
- storage/tests.rs: `conversations_snapshot_resolved_launch_and_switch_only_model_and_effort`, `conversations_in_an_earlier_format_stay_on_disk_but_are_hidden`, `streamed_events_wait_in_memory_until_the_batch_is_due`, `reconcile_takes_in_outside_changes_but_not_busy_conversations`, `write_failure_retains_events_and_explicit_retry_appends_once`, `failed_settings_write_preserves_pending_state_until_retry`, `bad_transcript_keeps_prior_history_and_exposes_error`, `a_torn_last_line_is_cut_off_and_the_conversation_stays_writable`, `unsafe_config_contents_block_delete_without_touching_workdir`, `forks_copy_visible_history_through_the_fork_point_only`
- engine/docs.rs: `only_files_are_read`, `a_save_replaces_the_whole_file_and_refuses_what_it_cannot_save`

**adeline (integration)**
- tests/engine.rs: `an_engine_killed_mid_turn_marks_the_turn_interrupted_and_retry_finishes_it`, `a_permission_answered_from_two_windows_counts_once`, `a_permission_asked_with_no_window_open_stops_the_turn_and_retry_continues_it`, `a_message_sent_during_a_turn_waits_in_the_queue_and_runs_next`, `a_queued_message_taken_back_keeps_its_attachments`, `a_reconnect_gets_only_the_deltas_it_missed_in_order`, `a_silent_agent_is_flagged_quiet_and_force_stop_frees_the_conversation`, `an_agent_that_exits_mid_turn_is_restarted_and_without_retries_its_stderr_is_shown`, `a_lost_session_waits_for_consent_and_the_replacement_carries_the_history`
- tests/headless.rs: `headless_cli`
- tests/ui.rs: `quit_background`, `quit_stop_all`, `quit_stop_all_shared`, `settings_engine`, `unavailable`, `fork`, `chat_from_another_client`, `docs_after_project_recreated`, `docs_refresh_after_leaving`, `docs_conflict_blocks_save`, `add_machine`, `drop_and_reconnect`, `ssh_failures`, `ssh_host_keys`, `install_twice_at_once`, `headless_remote`, `upgrade_and_browse`

**adeline_ui**
- tests/ui.rs: `demo_conversation`, `expanded_activity_row_shows_the_whole_command`, `chat_list_options_stay_open_while_switching`, `docs_demo`, `close_tab`, `right_island_resizes_in_every_mode`, `resizing_a_pane_selects_no_text`, `a_lost_mouse_up_ends_the_selection`, `selection_stays_in_panel`, `transcript_selection`, `docs_failed_rename_keeps_editing`, `docs_task_checkboxes_show_and_toggle`, `delete_agent`, `edit_an_agent_opens_the_open_chats_agent`
- tests/pixels.rs: `demo_chat_draws_in_theme_colors`, `selected_chat_row_shows`, `mermaid_diagram_renders`
- tests/pixels_windows.rs: `smoke`
- src/theme.rs: `bundled_themes_populate_fresh_config_and_preserve_existing_files`, `theme_names_cannot_leave_the_themes_folder`
- src/fonts.rs: `catalogue_includes_bundle_once_and_sorts_system_families`
- src/chat/list.rs: `labels_stack_under_the_controls`, `jumping_puts_the_first_chat_right_under_the_stack`, `keyboard_reveal_keeps_rows_clear_of_the_stack_and_in_view`
- src/docs.rs: `a_note_quotes_the_selection_with_its_path_and_lines`, `anchors_follow_github_heading_slugs`
- src/docs/viewer.rs: `task_bodies_dedent_continuation_lines`
- src/activity.rs: `replies_split_around_the_tool_calls_made_while_writing`
- src/themed_icon.rs: `inherited_icon_color_tracks_states_and_preserves_overrides`

## Duplicated across crates

- CRLF on save is tested three times: `adeline_docs` `text.rs` `saving_keeps_line_endings_and_the_final_newline_as_they_were`, `engine/docs.rs` `a_save_replaces_the_whole_file_and_refuses_what_it_cannot_save`, and engine.rs `docs_an_agent_writes_…`. Keep the `adeline_docs` one.
- Drop and reconnect: `headless_remote` repeats `drop_and_reconnect`. Keep only the headless-specific part.
- Mermaid render waits: `docs_diagrams_render_in_the_background` and pixels.rs `mermaid_diagram_renders`. Both stay, because only the first runs on Windows.

## Missing keyboard shortcuts

Tests click "Main menu" (`edit_an_agent_opens_the_open_chats_agent`) and the task checkbox ("Mark as not done") because no shortcut reaches them. AGENTS.md asks for a shortcut wherever a test has to click.

## Outcome (v0.3.8)

- The ten low-value tests were deleted. `launch`'s Windows GUI-subsystem check moved into `quit_idle`, and the unique-names check from `bundled_themes_load_with_all_tokens` became `bundled_theme_names_are_unique`.
- The mid-value tests were trimmed, split or strengthened. These were left as they were: `the_current_step_is_the_running_one_else_the_next_pending`, `ten_thousand_docs_filter_quickly`, `changing_retries_affects_the_running_turn`, `an_agent_on_another_protocol_version_gets_a_version_error`, `quit_idle`, `traffic_needs_verbose`, `missing_theme_falls_back_to_embedded_islands`, `default_legacy_empty_and_unknown_fonts_use_the_bundle`, `remeasuring_variable_rows_preserves_the_scroll_anchor`, `portrait_raster_matches_display_pixels` and `a_paragraph_quoted_on_every_line_unquotes`. `platform.rs` `owner_only_pipe_accepts_this_user` was deleted, because every engine test already connects through the pipe.
- The `tokens`, `duration` and `excerpt` bugs are fixed, and their tests cover the edges.
- High-value tests are unchanged, including the assertions above that can't fail.
- Still open:
  - `open_folder_name_taken` can't read the notification's text, because gpui-component's `Notification` has no accessibility label.
  - `copy_code_block` stays live, because demo mode shows replies as plain text.
  - No input found reaches mermaid's "diagram is empty" branch.
  - adeline_core's tokio dependency lacks `rt-multi-thread`, so `cargo test -p adeline_core` on its own doesn't build. Workspace builds work.
