# Native parity acceptance (#141)

What proves that each view and flow of the native app (`caskade-native`)
works by keyboard alone and by pad alone, is heard by screen readers,
follows a language switch and draws in every theme, in desktop and console
mode. Each cell names the tests that cover it, "manual" with the #120 row
or `docs/manual-checks.md` check that covers it on real hardware, or **gap**
when nothing does yet. The gaps are listed again at the end.

How to read the names:

- `area/file::test` is a headless test under `apps/native/src/`
  (`frame/tests::every_page_of_the_webview_opens` is in
  `apps/native/src/frame/tests.rs`). The folders of tests are left out:
  `settings/x::test` is `apps/native/src/settings/tests/x.rs`,
  `ui/x_tests::test` is `ui/tests/x_tests.rs`, `flows/x_tests::test` is
  `flows/tests/x_tests.rs`, and `game/page_tests::test` is
  `game/page_tests/mod.rs`.
- `tests/file::test` is an integration test in `apps/native/tests/`.
- `ui-i18n::test` and `ui-theme::test` are in `crates/caskade-ui-i18n` and
  `crates/caskade-ui-theme`.

How to run them is in [native-testing.md](native-testing.md). The window
tests (`smoke`, `console_e2e`, `backdrop`) are `#[ignore]`d and run with
`just test-native-window`; the screenshots need `--features shots`
(`just test-native`).

## Shared by every view

These hold for every page because the frame, the input layer and the text
lookup are shared; the matrix below names only what a view adds.

| What | Covered by |
| --- | --- |
| Focus engine: a pad and a keyboard move focus through a grid, the sidebar and a dialog the same way | `input/tests::a_pad_and_a_keyboard_each_work_the_grid_the_sidebar_and_a_dialog`, `input/tests::a_pad_move_waits_for_the_layout_of_what_just_changed` |
| Focus restoration | `input/tests::focus_checks_outlive_the_component_that_started_them`, `input/tests::focus_handed_back_to_a_control_that_went_does_nothing`, `ui/overlay_tests::a_menu_opens_moves_types_ahead_runs_and_gives_focus_back`, `frame/language_tests::an_open_dialog_follows_a_switch_and_keeps_its_focus` |
| Back (pad B, Escape) through the router's history | `frame/tests::the_sidebar_navigates_by_keyboard_and_pad`, `router/tests::history_goes_back_and_forward_and_replaces_in_place`, `router/tests::game_pages_go_back_only_to_app_pages_and_never_too_deep`, `ui/overlay_tests::a_confirm_dialog_starts_on_cancel_and_closes_on_escape_or_back` |
| Text fields, the edit menu, comboboxes | `ui/field_tests::*` (four tests), `input/field::a_cut_from_the_menu_is_heard_as_typing` |
| On-screen keyboard (a pad types) | `input/tests::a_click_focuses_the_control_without_a_ring_and_a_pad_types_in_a_field`; the keyboard's layout and keys in `crates/caskade-ui-input` (`osk`) |
| IME composition reaches a field; placeholders while composing | `tests/text_field::a_composition_reaches_the_fields_listeners`, `tests/text_display::a_placeholder_shows_while_the_field_is_empty_and_not_composing`. Real CJK input methods: manual, #120 (#84 rows) |
| Components as screen readers hear them (roles, names, states, actions, live regions) | `ui/button_tests::buttons_and_links_as_a_screen_reader_hears_them`, `ui/choice_tests::progress_tabs_badges_and_notices_as_a_screen_reader_hears_them`, `ui/game_tests::game_cards_and_rows_as_a_screen_reader_hears_them`, `ui/overlay_tests::a_menu_as_a_screen_reader_hears_it`, `ui/overlay_tests::a_dialog_as_a_screen_reader_hears_it`, `ui/showcase_tests::the_avatar_picker_as_a_screen_reader_hears_it`, `ui/toggle_tests::switches_and_checkboxes_as_a_screen_reader_hears_them`, `ui/setting_row_tests::a_setting_row_names_and_describes_its_controls` |
| The AccessKit tree follows the UI; screen-reader actions reach the handlers; announcements | `tests/accessibility::the_tree_has_roles_names_states_and_values`, `tests/accessibility::the_tree_follows_the_ui_and_actions_reach_the_handlers` |
| Every control on every page is named in the real app's AccessKit tree | `frame/a11y_tests::every_control_on_every_page_has_a_name_for_screen_readers` (Home, Library, a game page, Store, Collections, Activity, all 17 Settings sections, sidebar, top bar, status bar) |
| Real screen readers (Orca, Narrator, VoiceOver) | manual, #120 (#83 rows) |
| A language switch updates mounted text without a restart | `frame/language_tests::switching_to_swedish_updates_home_the_library_and_settings_in_place`, `frame/language_tests::screen_readers_hear_the_sidebar_and_settings_in_the_new_language`, `input/tests::switching_language_updates_the_open_guide_menu_without_a_restart` |
| No untranslated RSX text | `ui-i18n::every_rsx_text_people_see_goes_through_a_macro` |
| Pseudo-localized layout (`en-XA`) | `frame/language_tests::the_pseudo_locale_marks_the_frames_labels_and_keeps_them_laid_out`, `ui-i18n::the_pseudo_locale_accents_and_lengthens_text_in_debug_builds`; every view by eye: manual, `manual-checks.md` L1 |
| Every theme in `themes/` on the reference screens and the gallery | `tests/shots::every_theme_renders_the_reference_screens_as_before` (the frame on Home and the library grid, the library, the preview and the gallery's 15 pages, in all six themes), `tests/shots/borders::*` |
| Reduced motion and reduced effects | `theme::system_motion_follows_the_os`, `backdrop::appearance_reaches_the_gpu_through_the_tokens` (reduce effects hides the grid; reduced motion stops the backdrop's frames), `settings/appearance::the_look_cards_and_motion_read_and_save`. On a real display: manual, #120 (#148) |
| Contrast | `ui-theme` accent tests (`keeps_a_readable_accent_and_derives_its_tones`, `darkens_on_a_light_theme_and_refuses_what_cant_be_made_readable`), `settings/appearance::accents_save_kept_readable_and_go_back_to_the_themes`. A theme's own text-on-surface contrast: **gap** |
| CPU fallback | `main::only_wgpu_finding_no_renderer_stops_the_restarts`, `backdrop::the_sheets_stop_the_document_painting_or_moving_its_backdrop`, the screenshots (vello_cpu); CI's window tests on lavapipe. Real machines without a GPU: manual, #120 (#110) |
| Console mode as a whole, pad only | `tests/console_e2e::console_mode_end_to_end_with_a_pad_only` (Home → a shelf → a game → Play, guide → back, guide → stop, Library → a ROM game, Settings → a switch, out of console mode), `tests/console_e2e::a_scripted_pad_drives_the_native_app_with_no_key_or_pointer_event` (sidebar → Collections → Back, guide with nothing running). Real pads, TV, Steam Deck: manual, #120 (#107), `manual-checks.md` W10, H2, H3 |

## The matrix

Columns: **Keyboard** and **Pad** (operated with only that), **Focus**
(where focus starts, comes back to, and what Back does), **Text** (typing,
the on-screen keyboard), **Guard** (unsaved changes), **AccessKit**
(beyond the shared names check above), **Language** (a switch updates the
mounted view), **Themes** (screenshots in every theme), **Console** (the
view in console mode).

| View or flow | Keyboard | Pad | Focus | Text | Guard | AccessKit | Language | Themes | Console |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Onboarding (first-run setup) | `onboarding/tests::the_choices_and_the_admins_account_are_saved` | **gap** | **gap** | `onboarding/tests::the_choices_and_the_admins_account_are_saved` | n/a | `onboarding/tests::a_first_run_goes_through_the_questions_and_the_work_to_the_library` (progress label) | **gap** | **gap** (not a reference screen) | `onboarding/tests::choosing_console_mode_switches_the_window_at_once` |
| Accounts: picker, PIN, age gate | **gap** | `accounts/tests::the_picker_in_console_mode_has_larger_tiles_and_the_hint_bar` | **gap** | **gap** (PIN by keys or pad) | n/a | `accounts/tests::switch_account_signs_out_to_the_picker_and_a_locked_account_asks_for_its_pin` (DOM roles) | **gap** | `tests/shots` (gallery accounts page) | `accounts/tests::the_picker_in_console_mode_has_larger_tiles_and_the_hint_bar` |
| Frame: sidebar, top bar, search, status bar | `frame/tests::the_sidebar_navigates_by_keyboard_and_pad`, `frame/tests::the_search_suggests_games_and_opens_the_library_on_the_text` | `frame/tests::the_sidebar_navigates_by_keyboard_and_pad`, `tests/console_e2e::a_scripted_pad_drives_the_native_app_with_no_key_or_pointer_event` | `settings/tests::navigating_from_the_sidebar_puts_pad_focus_on_the_open_settings_tab` | `frame/tests::typing_outside_a_field_opens_the_search_overlay` | n/a | `frame/language_tests::screen_readers_hear_the_sidebar_and_settings_in_the_new_language` | `frame/language_tests::switching_to_swedish_updates_home_the_library_and_settings_in_place` | `tests/shots` (`shell`, `home`) | `console/tests::view_switches_to_console_mode_and_back` |
| Home | **gap** (shared focus engine only) | `home/tests::lt_and_rt_page_focus_along_a_shelf`, `tests/console_e2e::console_mode_end_to_end_with_a_pad_only` | `home/tests::the_game_played_last_is_the_hero_and_leaves_recently_played` | n/a | n/a | names check | `frame/language_tests::switching_to_swedish_updates_home_the_library_and_settings_in_place` | `tests/shots` (`home`, gallery home) | `tests/console_e2e::console_mode_end_to_end_with_a_pad_only` |
| Library filters | `frame/tests::the_sidebar_navigates_by_keyboard_and_pad` | `library/view_tests::lb_and_rb_step_through_the_filters_and_wrap` | `frame/tests::the_sidebar_navigates_by_mouse_with_the_librarys_filters_and_counts` (Back) | `library/view_tests::a_search_shows_as_a_chip_that_clears_it` | n/a | names check; filter list's name in `frame/language_tests::switching_to_swedish_updates_home_the_library_and_settings_in_place` | the same | `tests/shots` (`library`) | `library/platform_tests::emulated_games_show_by_platform_and_a_platform_opens_its_games` |
| Library grid and list | `library/view_tests::the_sort_menu_and_the_lists_headers_sort_the_library` | `library/tests::a_pad_pages_through_the_grid_and_focus_follows`, `library/tests::scrolling_the_list_keeps_its_rows_bounded` | `library/window::the_focused_row_stays_mounted_and_its_page_loaded`, `library/window::new_columns_keep_the_first_game_shown_at_the_top_and_focus_in_view` | n/a | n/a | names check; `ui/game_tests::game_cards_and_rows_as_a_screen_reader_hears_them` | shared | `tests/shots` (`library`, `shell`) | `tests/console_e2e::console_mode_end_to_end_with_a_pad_only` |
| Library selection and bulk edit | `library/selection_tests::chosen_games_take_one_edit_after_a_preview` | `library/selection_tests::games_are_chosen_by_click_and_range_and_all_of_them` | the same (Back clears the choice) | `library/selection_tests::chosen_games_take_one_edit_after_a_preview` | **gap** (bulk edit) | names check | shared | **gap** | **gap** |
| Library menus, quick look | `ui/overlay_tests::a_menu_opens_moves_types_ahead_runs_and_gives_focus_back` | `game_menu/tests::a_pad_opens_the_focused_games_menu_and_quick_look` | `ui/overlay_tests::a_menu_opens_moves_types_ahead_runs_and_gives_focus_back` | `game_menu/tests::add_to_collection_makes_a_collection_with_the_game_in_it` | n/a | `ui/overlay_tests::a_menu_as_a_screen_reader_hears_it` | shared | `tests/shots` (gallery menu) | `game_menu/tests::a_pad_opens_the_focused_games_menu_and_quick_look` |
| Game page | `game/page_tests/page_data_tests::a_release_with_several_launch_actions_picks_the_one_play_starts_and_lists_its_tools` | `tests/console_e2e::console_mode_end_to_end_with_a_pad_only` (a game → Play, a ROM game) | `game/page_tests::back_goes_where_the_game_was_opened_from` | n/a | n/a | names check | shared | **gap** (not a reference screen) | `game/page_tests/page_data_tests::a_fan_port_shows_what_it_needs_and_installs_after_asking` |
| Game editor | `game/editor/tests::closing_with_unsaved_changes_asks_first` | `game/editor/tests::edit_details_saves_the_changed_fields_as_one_patch_and_closes`, `game/editor/tests::closing_with_unsaved_changes_asks_first` | `game/editor/tests::closing_with_unsaved_changes_asks_first` | the same | `game/editor/tests::closing_with_unsaved_changes_asks_first` | **gap** (not in the page walk: it's a dialog) | shared | **gap** | **gap** |
| Store | `store/tests::search_waits_for_typing_to_stop_and_keeps_the_text_in_the_route` | `store/tests::lb_and_rb_switch_the_stores_tabs`, `store/game_tests::igdbs_videos_play_in_the_video_window_and_its_screenshots_fill_the_gallery` | `store/game_tests::a_catalog_game_opens_from_the_results_and_back_returns_to_them` | `store/tests::search_waits_for_typing_to_stop_and_keeps_the_text_in_the_route` | n/a | names check | shared | **gap** | **gap** (CI's e2e can't reach the catalogue) |
| Collections | `collections/tests::makes_a_manual_collection_and_adds_games_to_it` | `collections/tests::lists_collections_pinned_first_and_opens_one_with_its_games`, `tests/console_e2e::a_scripted_pad_drives_the_native_app_with_no_key_or_pointer_event` | `collections/tests::plays_a_random_installed_game_from_the_collection`, `frame/language_tests::an_open_dialog_follows_a_switch_and_keeps_its_focus` | `collections/tests::makes_a_manual_collection_and_adds_games_to_it` | n/a | names check | `frame/language_tests::an_open_dialog_follows_a_switch_and_keeps_its_focus` (page, menu's dialog) | **gap** | **gap** |
| Activity and its questions | `activity/tests::a_job_waiting_for_an_answer_opens_its_question_and_continues`, `activity/tests::closing_a_copy_from_another_device_dismisses_it` | `activity/tests::a_download_pauses_and_resumes_by_mouse_keyboard_pad_and_menu`, `activity/tests::a_deep_link_waits_until_confirmed_and_its_job_shows` | `activity/tests::closing_a_copy_from_another_device_dismisses_it` (Back) | `activity/tests::a_link_pasted_outside_a_field_asks_to_install_it` | n/a | names check; `frame/tests::the_activity_label_says_the_running_job_and_what_waits` | **gap** | `tests/shots` (gallery feedback) | **gap** |
| Settings: sections, search | `settings/tests::opens_a_section_in_place_by_click_arrows_and_a_pads_bumpers`, `settings/tests::search_opens_the_first_result_and_highlights_its_row` | `settings/tests::opens_a_section_in_place_by_click_arrows_and_a_pads_bumpers`, `settings/tests::typing_and_a_pads_search_go_to_settings_search` | `settings/tests::navigating_from_the_sidebar_puts_pad_focus_on_the_open_settings_tab` | `settings/tests::typing_and_a_pads_search_go_to_settings_search` | `settings/tests::leaving_a_section_with_unsaved_edits_asks_and_does_what_the_answer_says` | `settings/tests::lists_every_section_in_groups_each_tab_tied_to_its_panel`, `frame/language_tests::screen_readers_hear_the_sidebar_and_settings_in_the_new_language` | `frame/language_tests::switching_to_swedish_updates_home_the_library_and_settings_in_place` | `tests/shots` (gallery settings) | `settings/tests::opens_a_section_in_place_by_click_arrows_and_a_pads_bumpers` |
| Settings → General | `settings/general::changes_save_by_keyboard_and_pad` | the same | shared | `settings/general::the_reset_puts_back_every_preference_on_the_page` | settings guard | `settings/general::every_control_is_named_by_its_row` | shared; the Language setting's choices: `settings/general::the_language_choices_keep_one_this_version_lacks` | gallery settings | `settings/general::every_field_reads_what_its_section_saved` |
| Settings → Appearance | `settings/appearance::lists_every_theme_and_arrows_pick_the_next` | `settings/appearance::the_look_cards_and_motion_read_and_save` | shared | `settings/appearance::custom_css_saves_refuses_and_switches_off` | `settings/appearance::accents_save_kept_readable_and_go_back_to_the_themes` | `settings/appearance::every_control_is_named` | shared | the themes themselves: `tests/shots`; picking each on a real GPU: manual, #120 (#104) | **gap** |
| Settings → Plugins (and the catalogue) | `settings/plugins::a_plugin_is_reviewed_allowed_turned_off_and_uninstalled`, `settings/addons::lists_searches_and_filters_the_catalogue` | **gap** | shared | `settings/addons::lists_searches_and_filters_the_catalogue` | n/a | names check | shared | **gap** | **gap** |
| Settings → Library Discovery | `settings/discovery::the_preferences_read_and_save_and_off_stops_scans` | the same | shared | `settings/discovery::folders_are_added_and_removed` | n/a | names check | shared | **gap** | **gap** |
| Settings → Launchers, Installer | `settings/launchers::lists_the_found_launchers_and_switches_them`, `settings/launchers::torrent_links_open_with_caskade_and_go_back` | `settings/launchers::lists_the_found_launchers_and_switches_them` | shared | n/a | n/a | names check | shared | **gap** | **gap** |
| Settings → Store accounts | `settings/stores::connects_disconnects_and_removes_an_account` | **gap** | shared | the same | n/a | names check | shared | **gap** | **gap** |
| Settings → Storage | `settings/storage::adds_a_folder_and_makes_another_location_the_default` | **gap** | shared | the same | `settings/storage::lists_the_locations_selects_one_and_saves_its_label_and_template` | names check | shared | **gap** | **gap** |
| Settings → Emulation | **gap** (mouse only) | **gap** | shared | **gap** | n/a | names check | shared | **gap** | **gap** |
| Settings → Scripts | `settings/scripts::a_script_is_added_ordered_duplicated_and_deleted` | **gap** | shared | the same | n/a | names check | shared | **gap** | **gap** |
| Settings → Devices, Streaming | `settings/devices::this_device_is_renamed_and_its_connections_switch`, `settings/streaming::hosting_turns_on_and_a_waiting_device_pairs_with_its_code` | **gap** | shared | `settings/streaming::a_device_to_stream_from_pairs_through_the_dialogs_code` | n/a | names check | shared | **gap** | **gap** |
| Settings → My profile, Accounts | `settings/accounts::an_admin_adds_changes_and_deletes_accounts`, `settings/accounts::my_profile_saves_its_name_and_sets_its_pin` | **gap** | shared | the same | settings guard (`settings/tests::leaving_a_section_with_unsaved_edits_asks_and_does_what_the_answer_says` uses My profile) | names check | shared | gallery accounts | **gap** |
| Settings → Backup, Statistics, About | `settings/backup::a_folder_that_isnt_there_blocks_the_backup_with_why` | **gap** | shared | n/a | n/a | names check; `settings/about::about_shows_the_build_credits_and_opens_legal_in_place` | shared | **gap** | **gap** |
| Sharing and receiving a game | `flows/share_tests::a_link_is_looked_at_and_the_game_received_from_it` | **gap** | **gap** | the same | n/a | **gap** (dialogs) | **gap** | **gap** | **gap** |
| Pairing (devices, streaming hosts) | `flows/host_tests::a_device_asking_to_stream_is_prompted_and_paired_with_the_code_it_shows` | **gap** | **gap** | the same | n/a | **gap** (dialogs) | **gap** | **gap** | **gap** |
| Storage flows: install location, move | `flows/storage_tests::a_game_installed_from_a_link_goes_where_the_user_chose` | **gap** | **gap** | the same | n/a | **gap** (dialogs) | **gap** | **gap** | **gap** |
| Add a game, file and folder picker | `flows/add_game_tests::a_game_by_title_is_added_and_opened`, `flows/picker_tests::the_browser_opens_folders_goes_up_and_picks_the_folder_shown` | `flows/picker_tests::a_pad_picks_the_folder_with_start_and_cancels_with_b` | `flows/picker_tests::a_pad_picks_the_folder_with_start_and_cancels_with_b` | `flows/add_game_tests::a_uri_is_resolved_summarised_added_and_then_offered_to_join` | n/a | **gap** (dialogs) | **gap** | **gap** | the picker is console mode's folder browser |
| Play flows (newer release, other device's save, busy host) | **gap** | **gap** | **gap** | n/a | n/a | **gap** (dialogs) | **gap** | **gap** | **gap** |
| Guide and the quick menu | n/a (a pad's button) | `input/tests::guide_opens_the_quick_menu_over_a_running_game_and_return_goes_back_to_it`, `input/tests::stopping_asks_first_by_pad_then_stops_the_session`, `input/tests::the_menu_opens_the_games_details_and_back_or_guide_again_closes_it` | the same | n/a | n/a | **gap** | `input/tests::switching_language_updates_the_open_guide_menu_without_a_restart` | **gap** | `console/tests::the_curtain_comes_down_over_a_running_game_and_lifts`, `tests/console_e2e::console_mode_end_to_end_with_a_pad_only` |
| Plugin UI (views, sections, actions, badges) | **gap** | `plugins/tests::a_view_draws_tells_the_plugin_and_keeps_its_state` | **gap** | n/a | n/a | `plugins/tests::drawn_stats_read_as_a_list_of_values` | `frame/language_tests::plugins_contributions_are_asked_again_in_the_new_language`; a drawing itself: **gap** (`frame/language_tests::a_plugins_drawing_is_asked_again_in_the_new_language`, ignored) | **gap** | **gap** |

## Gaps

What no test covers yet, in the order they matter for #141:

1. **A plugin's drawing doesn't follow a language switch.** `DrawnOne`'s
   tree (`apps/native/src/plugins/mod.rs`) is a resource that restarts on
   its state and the mode, not the language, so a contribution's title
   changes but what it draws stays in the old language until something
   else redraws it. `frame/language_tests::a_plugins_drawing_is_asked_again_in_the_new_language`
   is written and `#[ignore]`d until the resource also reads
   `crate::i18n::LANGUAGE`.
2. **Pad-only Settings panels.** Plugins, Store accounts, Storage,
   Emulation, Scripts, Devices, Streaming, My profile, Accounts, Backup,
   Statistics and About are proved by keyboard (and the shared focus
   engine), not by a pad driving their own controls.
3. **Flows by pad and keyboard.** Sharing, pairing, storage moves and the
   play questions are tested by mouse or by calling their buttons; no test
   drives them with a pad only or Back out of them.
4. **Onboarding and the account picker by pad, and PIN entry by keys or
   pad.**
5. **Home by keyboard alone** (the shared focus engine covers it, but no
   Home test presses keys).
6. **AccessKit for dialogs and flows.** The names check walks pages, not
   open dialogs (editor, share, pairing, storage, play questions).
7. **Language switch in Activity, onboarding, flows' dialogs, toasts and
   notifications.** Toasts and errors already shown keep their words (they
   are strings made when shown, as in the webview); new ones use the new
   language.
8. **Screenshots of the game page, Store, Collections, Activity,
   onboarding, editor and the Settings sections**, and of console mode,
   reduced motion and the pseudo-locale: the reference screens are the
   frame, Home, the library, the preview and the gallery.
9. **A theme's own text-on-surface contrast**: accents are checked, the
   themes' text and surface tokens aren't.

## Manual rows

Real displays, GPUs, screen readers, input methods, pads and other OSes are
#120's rows; `docs/manual-checks.md` has the steps. The native app is
`caskade-native` (`cargo build -p caskade-native`, or the release bundle).
