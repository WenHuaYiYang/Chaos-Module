use super::*;
use crate::mock;

#[test]
fn dimmer_keyboard_only_exposes_digits_and_edit_actions() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        mock::reset();
        open(b"15\0".as_ptr(), Completion::DimmerTimeout);
        let parent = fw_api::obj_create(0, 0);
        assert!(render(parent));
        let codes: Vec<u32> = mock::with(|ui| ui.objects.iter().filter_map(|o| o.code).collect());
        assert_eq!(codes.len(), 13);
        for c in b'0'..=b'9' { assert!(codes.contains(&(c as u32))); }
        for c in [KEY_BACKSPACE, KEY_CLEAR, KEY_DONE] { assert!(codes.contains(&c)); }
        assert!(!chinese_capable());
        assert!(!symbols_enabled());
    }
}

// 横滑键盘：parent -> viewport -> board(640 宽)。setup 返回 (parent, board)。
// 中文页候选条固定在 parent 上（y=56），不在 viewport 里。
unsafe fn setup(completion: Completion) -> (u32, u32) {
    mock::reset();
    NSH_DRAFT.fill(0);
    open(core::ptr::null(), completion);
    let parent = fw_api::obj_create(0, 0);
    fw_api::obj_set_size(parent, 336, 424);
    assert!(render(parent));
    let board = board_of(viewport_of(parent));
    (parent, board)
}

// 重绘会追加对象，最后一个 viewport / board / content 才是当前布局。
// 视口高度随候选条出现与否变化（174 / 232），按宽度认。
unsafe fn viewport_of(parent: u32) -> u32 {
    mock::with(|ui| {
        ui.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.parent == parent && o.size.0 == VIEWPORT_WIDTH)
            .last()
            .unwrap()
            .0 as u32
            + 1
    })
}

unsafe fn board_of(viewport: u32) -> u32 {
    mock::with(|ui| {
        ui.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.parent == viewport && o.size.0 == BOARD_WIDTH)
            .last()
            .unwrap()
            .0 as u32
            + 1
    })
}

// 固定在页面上（不在视口里）的候选条：6 个候选 + 「更多」，取最近一次 render 的 7 个。
unsafe fn candidate_bar(parent: u32) -> Vec<mock::Object> {
    mock::with(|ui| {
        let all: Vec<_> = ui
            .objects
            .iter()
            .filter(|o| {
                o.parent == parent
                    && o.code.is_some()
                    && (o.size == (CAND_BAR_W, CAND_BAR_HEIGHT)
                        || o.size == (CAND_MORE_W, CAND_BAR_HEIGHT))
            })
            .cloned()
            .collect();
        all[all.len() - (CAND_BAR + 1)..].to_vec()
    })
}

// 候选面板的 content 是 viewport 的最后一个子对象。
unsafe fn panel_content_of(viewport: u32) -> u32 {
    mock::with(|ui| {
        ui.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.parent == viewport)
            .last()
            .unwrap()
            .0 as u32
            + 1
    })
}

unsafe fn panel_cells(content: u32) -> Vec<mock::Object> {
    mock::with(|ui| {
        ui.objects
            .iter()
            .filter(|o| o.parent == content && o.code.is_some())
            .cloned()
            .collect()
    })
}

unsafe fn board_keys(board: u32) -> Vec<mock::Object> {
    mock::with(|ui| {
        ui.objects
            .iter()
            .filter(|o| o.parent == board && o.code.is_some())
            .cloned()
            .collect()
    })
}

unsafe fn event(code: u32, event_code: u32) {
    mock::with(|ui| {
        ui.key_code = code;
        ui.event_code = event_code;
    });
    key_event(1);
}
unsafe fn press(code: u32) {
    event(code, fw_api::ev::CLICKED);
}
unsafe fn input() -> Vec<u8> {
    TEXT[..LENGTH].to_vec()
}

/// 点「中」后 firmware 会重建页；harness 不会自动重绘，这里手动补一次。
unsafe fn setup_chinese(completion: Completion) -> u32 {
    let (parent, _) = setup(completion);
    press(KEY_CHINESE);
    assert!(render(parent));
    parent
}

unsafe fn action_codes(parent: u32) -> Vec<u32> {
    mock::with(|ui| {
        // 重绘会追加对象；只看最近一次 render 的 viewport 之后的操作键，
        // 否则会把上一次布局里的 KEY_SHIFT 当成当前的 KEY_COLLAPSE。
        let viewport_idx = ui
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.parent == parent && o.size.0 == VIEWPORT_WIDTH)
            .last()
            .unwrap()
            .0;
        ui.objects
            .iter()
            .enumerate()
            .filter(|(i, o)| *i > viewport_idx && o.parent == parent && o.code.is_some())
            .map(|(_, o)| o.code.unwrap())
            .collect()
    })
}

fn assert_key_labels_centered() {
    mock::with(|ui| {
        let mut count = 0;
        for (i, key) in ui
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.code.is_some())
        {
            let labels: Vec<_> = ui
                .objects
                .iter()
                .filter(|o| o.parent == i as u32 + 1)
                .collect();
            assert_eq!(labels.len(), 1, "key {:?} must have one label", key.code);
            // 固件 0x79360 把 9 派发到 center/center，旧值 8 是 right/center。
            assert_eq!(labels[0].alignment, 9, "key {:?} must be centered", key.code);
            assert_eq!(labels[0].pos, (0, 0));
            count += 1;
        }
        assert_eq!(count, 43); // 36 字符 + SHIFT/SPACE/退格/CLR/句点/中/OK
    });
}

#[test]
fn every_key_label_uses_firmware_center_alignment() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::DeviceName);
        assert_key_labels_centered();
        assert_eq!(mock::object(INPUT_LABEL).alignment, fw_api::align::TOP_LEFT);
        assert_eq!(mock::object(INPUT_LABEL).pos, (8, INPUT_TOP));
    }
}

#[test]
fn full_qwerty_rows_are_ordered_staggered_and_fit_the_scroll_board() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (_, board) = setup(Completion::DeviceName);
        let keys = board_keys(board);
        assert_eq!(keys.len(), 36);
        let mut unique = std::collections::BTreeSet::new();
        for (row, expected) in [
            b"1234567890".as_slice(),
            b"qwertyuiop",
            b"asdfghjkl",
            b"zxcvbnm",
        ]
        .iter()
        .enumerate()
        {
            let row_keys: Vec<_> = keys
                .iter()
                .filter(|k| k.pos.1 == row as i32 * ROW_STEP)
                .collect();
            assert_eq!(
                row_keys.iter().map(|k| k.code.unwrap() as u8).collect::<Vec<_>>(),
                *expected
            );
            // 每行在 640px 键盘里居中：10/10/9/7 键的起点。
            assert_eq!(row_keys[0].pos.0, [4, 4, 36, 100][row]);
            for pair in row_keys.windows(2) {
                assert_eq!(pair[1].pos.0 - pair[0].pos.0, KEY_WIDTH + KEY_GAP);
            }
        }
        for key in keys {
            assert!(unique.insert(key.code.unwrap()));
            assert_eq!(key.size, (KEY_WIDTH, KEY_HEIGHT));
            assert!(key.spacing_cleared);
            assert_eq!(key.flags & fw_api::LV_OBJ_FLAG_SCROLLABLE, 0);
            assert!(key.pos.0 >= 0 && key.pos.0 + key.size.0 <= BOARD_WIDTH);
            assert!(key.pos.1 >= 0 && key.pos.1 + key.size.1 <= BOARD_HEIGHT_FULL);
        }
    }
}

#[test]
fn keyboard_scrolls_horizontally_and_actions_fit_above_the_gesture_area() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (parent, board) = setup(Completion::SystemVersion);
        // parent 的直接子对象只有 输入框 + viewport + 7 个操作键。
        let children = mock::with(|ui| {
            ui.objects.iter().filter(|o| o.parent == parent).count()
        });
        assert_eq!(children, 9);
        let viewport = viewport_of(parent);
        assert_eq!(mock::object(viewport).size, (VIEWPORT_WIDTH, VIEWPORT_HEIGHT_PLAIN));
        assert_eq!(mock::object(viewport).pos, (0, VIEWPORT_TOP_PLAIN));
        assert_eq!(mock::object(viewport).scroll_dir, fw_api::LV_DIR_HOR);
        assert_ne!(mock::object(viewport).flags & fw_api::LV_OBJ_FLAG_SCROLLABLE, 0);
        assert_eq!(mock::object(viewport).flags & fw_api::LV_OBJ_FLAG_SCROLL_CHAIN, 0);
        assert_eq!(mock::object(viewport).flags & fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE, 0);
        assert_eq!(mock::object(viewport).flags & fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC, 0);
        assert_eq!(mock::object(parent).flags & fw_api::LV_OBJ_FLAG_SCROLLABLE, 0);
        assert_eq!(mock::object(board).parent, viewport);
        assert_eq!(mock::object(board).size, (BOARD_WIDTH, BOARD_HEIGHT_FULL));
        assert_eq!(mock::object(board).pos, (0, 0));
        assert_eq!(mock::object(board).flags & fw_api::LV_OBJ_FLAG_SCROLLABLE, 0);
        assert_eq!(mock::object(INPUT_LABEL).parent, parent);
        let actions = mock::with(|ui| {
            ui.objects
                .iter()
                .filter(|o| o.parent == parent && o.code.is_some())
                .cloned()
                .collect::<Vec<_>>()
        });
        assert_eq!(actions.len(), 7);
        assert_eq!(
            actions.iter().map(|o| o.code.unwrap()).collect::<Vec<_>>(),
            [KEY_SHIFT, b' ' as u32, KEY_BACKSPACE, KEY_CLEAR, b'.' as u32, KEY_CHINESE, KEY_DONE]
        );
        for action in &actions {
            assert!(action.pos.0 >= 0 && action.pos.0 + action.size.0 <= 336);
            assert!(action.pos.1 >= ACTION_TOP_PLAIN);
            assert!(action.pos.1 + action.size.1 <= 424 - 32);
        }
        for (i, a) in actions.iter().enumerate() {
            for b in &actions[i + 1..] {
                assert!(
                    a.pos.0 + a.size.0 <= b.pos.0
                        || b.pos.0 + b.size.0 <= a.pos.0
                        || a.pos.1 + a.size.1 <= b.pos.1
                        || b.pos.1 + b.size.1 <= a.pos.1
                );
            }
        }
    }
}

#[test]
fn shift_updates_every_letter_without_rebuilding() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::DeviceName);
        press(b'q' as u32);
        let count = mock::with(|ui| ui.objects.len());
        press(KEY_SHIFT);
        assert_eq!(mock::with(|ui| ui.objects.len()), count);
        for (i, &label) in LETTER_LABELS.iter().enumerate() {
            assert_eq!(mock::object(label).text, [b'a' + i as u8]);
        }
        assert_eq!(mock::object(SHIFT_LABEL).text, b"abc");
        assert_key_labels_centered();
        press(b'p' as u32);
        press(KEY_SHIFT);
        for (i, &label) in LETTER_LABELS.iter().enumerate() {
            assert_eq!(mock::object(label).text, [b'A' + i as u8]);
        }
        assert_eq!(mock::object(SHIFT_LABEL).text, b"ABC");
        assert_key_labels_centered();
        assert_eq!(input(), b"Qp");
    }
}

#[test]
fn editing_preserves_spaces_digits_periods_and_the_capacity_limit() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::DeviceModel);
        for code in b"p67 pro 1.0" {
            press(*code as u32);
        }
        assert_eq!(input(), b"P67 PRO 1.0");
        press(KEY_BACKSPACE);
        assert_eq!(input(), b"P67 PRO 1.");
        press(KEY_CLEAR);
        press(KEY_BACKSPACE);
        assert!(input().is_empty());
        for _ in 0..CAPACITY + 5 {
            press(b'x' as u32);
        }
        assert_eq!(input(), vec![b'X'; CAPACITY]);
        assert_eq!(TEXT[CAPACITY], 0);
        assert_eq!(mock::object(INPUT_LABEL).text, input());
        press(KEY_BACKSPACE);
        press(b'0' as u32);
        assert_eq!(input().last(), Some(&b'0'));
        assert_eq!(TEXT[CAPACITY], 0);
    }
}

#[test]
fn non_click_events_and_inactive_pages_never_type_or_save() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::SystemText);
        for event_code in [1, 2, 8, 9, 10, 11, 12, 30] {
            event(b'q' as u32, event_code);
            event(KEY_DONE, event_code);
        }
        FG_PAGE = 0;
        press(b'x' as u32);
        FG_PAGE = PAGE_PID;
        mock::with(|ui| ui.live = false);
        press(b'x' as u32);
        press(KEY_SHIFT);
        assert!(UPPERCASE);
        assert!(input().is_empty());
        assert!(mock::with(|ui| ui.saves.is_empty()));
    }
}

#[test]
fn every_completion_saves_the_buffer_and_only_success_navigates_back() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        for (index, completion) in [
            Completion::SystemVersion,
            Completion::SystemText,
            Completion::DeviceName,
            Completion::DeviceModel,
        ]
        .into_iter()
        .enumerate()
        {
            setup(completion);
            press(KEY_DONE);
            assert!(mock::with(|ui| ui.saves.is_empty()));
            for c in b"1.0" {
                press(*c as u32);
            }
            mock::with(|ui| ui.save_ok = false);
            press(KEY_DONE);
            assert_eq!(mock::with(|ui| ui.back_count), 0);
            mock::with(|ui| ui.save_ok = true);
            press(KEY_DONE);
            assert_eq!(
                mock::with(|ui| ui.saves.last().cloned()),
                Some((index as u32, b"1.0".to_vec()))
            );
            assert_eq!(mock::with(|ui| ui.back_count), 1);
        }
    }
}

#[test]
fn reopen_and_failed_render_discard_old_label_handles() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (parent, _) = setup(Completion::DeviceName);
        press(KEY_SHIFT);
        open(b"Original text\0".as_ptr(), Completion::SystemText);
        assert!(UPPERCASE);
        assert_eq!(input(), b"Original text");
        assert_eq!(INPUT_LABEL, 0);
        assert_eq!(LETTER_LABELS, [0; 26]);
        assert_eq!(SHIFT_LABEL, 0);
        mock::with(|ui| ui.fail_at = Some(ui.objects.len() + 1));
        assert!(!render(parent));
        assert_eq!(INPUT_LABEL, 0);
        assert_eq!(LETTER_LABELS, [0; 26]);
        mock::with(|ui| ui.fail_at = None);
        assert!(render(parent));
        assert_eq!(mock::object(INPUT_LABEL).text, b"Original text");
        assert!(LETTER_LABELS.iter().all(|&label| label != 0));
    }
}

#[test]
fn nsh_mode_is_lowercase_has_255_bytes_and_keeps_tail_visible() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::NshCommand);
        assert!(!UPPERCASE);
        assert_eq!(capacity(), 255);
        for c in b"ls /data -la" { press(*c as u32); }
        assert_eq!(input(), b"ls /data -la");
        press(KEY_CLEAR);
        for _ in 0..260 { press(b'a' as u32); }
        assert_eq!(LENGTH, 255);
        assert_eq!(TEXT[255], 0);
        assert_eq!(mock::object(INPUT_LABEL).text, vec![b'a'; 20]);
        let initial = [b'z'; 256];
        open(initial.as_ptr(), Completion::DeviceName);
        assert_eq!(capacity(), 31);
        assert_eq!(input(), vec![b'z'; 31]);
    }
}

#[test]
fn nsh_symbol_page_covers_all_punctuation_and_preserves_input() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (parent, _) = setup(Completion::NshCommand);
        press(b'l' as u32); press(b's' as u32);
        press(KEY_SYMBOLS);
        assert!(SYMBOL_MODE);
        assert_eq!(mock::with(|ui| ui.render_requests.clone()), [23]);
        let first = mock::with(|ui| ui.objects.len());
        assert!(render(parent));
        let chars = mock::with(|ui| ui.objects[first..].iter().filter_map(|o| o.code)
            .filter(|&c| c < 256).map(|c| c as u8).collect::<std::collections::BTreeSet<_>>());
        for c in 0x21u8..=0x7e { if c.is_ascii_punctuation() { assert!(chars.contains(&c), "missing {c}"); } }
        mock::with(|ui| {
            for (i, key) in ui.objects.iter().enumerate().skip(first).filter(|(_, o)| o.code.is_some()) {
                let label = ui.objects.iter().find(|o| o.parent == i as u32 + 1).unwrap();
                assert_eq!(label.alignment, 9, "key {:?}", key.code);
                assert_eq!(label.pos, (0, 0));
            }
        });
        assert_eq!(input(), b"ls");
        press(b'/' as u32); press(KEY_SYMBOLS);
        assert!(!SYMBOL_MODE); assert_eq!(input(), b"ls/");
    }
}

#[test]
fn nsh_submission_failure_preserves_input_and_success_cannot_double_send() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::NshCommand);
        for c in b"echo ok" { press(*c as u32); }
        mock::with(|ui| ui.save_ok = false);
        press(KEY_DONE);
        assert_eq!(input(), b"echo ok");
        assert_eq!(mock::with(|ui| ui.back_count), 0);
        assert_eq!(mock::object(INPUT_LABEL).text, "发送失败，请重试".as_bytes());
        mock::with(|ui| ui.save_ok = true);
        press(KEY_DONE); press(KEY_DONE);
        assert_eq!(mock::with(|ui| ui.back_count), 1);
        assert_eq!(mock::with(|ui| ui.saves.clone()), [(4, b"echo ok".to_vec()), (4, b"echo ok".to_vec())]);
    }
}

#[test]
fn nsh_empty_submission_is_allowed_but_setting_empty_is_not() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::NshCommand); press(KEY_DONE);
        assert_eq!(mock::with(|ui| ui.saves.clone()), [(4, vec![])]);
        setup(Completion::SystemVersion); press(KEY_DONE);
        assert!(mock::with(|ui| ui.saves.is_empty()));
    }
}

#[test]
fn nsh_draft_survives_cancel_and_other_settings_but_success_clears_it() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::NshCommand);
        for c in b"echo draft" { press(*c as u32); }
        FG_PAGE = 31; // Native title/back gesture; no submission callback.
        open(core::ptr::null(), Completion::NshCommand);
        assert_eq!(input(), b"echo draft");
        open(b"custom\0".as_ptr(), Completion::DeviceName);
        open(core::ptr::null(), Completion::NshCommand);
        assert_eq!(input(), b"echo draft");
        press(KEY_DONE);
        open(core::ptr::null(), Completion::NshCommand);
        assert_eq!(input(), b"");
        open(b"initial draft\0".as_ptr(), Completion::NshCommand);
        open(core::ptr::null(), Completion::NshCommand);
        assert_eq!(input(), b"initial draft");
    }
}

#[test]
fn web_password_is_lowercase_supports_symbols_and_stops_at_31_bytes() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (parent, _) = setup(Completion::WebPassword);
        assert_eq!(title(), "网页服务器密码\0".as_bytes());
        assert!(!UPPERCASE);
        assert_eq!(capacity(), 31);
        for c in b"pass word" { press(*c as u32); }
        press(KEY_SYMBOLS);
        assert!(SYMBOL_MODE);
        assert!(render(parent));
        for c in b"!@#$%" { press(*c as u32); }
        assert_eq!(input(), b"pass word!@#$%");
        for _ in 0..40 { press(b'x' as u32); }
        assert_eq!(LENGTH, 31);
        assert_eq!(TEXT[31], 0);
        assert_eq!(mock::object(INPUT_LABEL).text, input()[11..]);
        press(KEY_DONE);
        assert_eq!(mock::with(|ui| ui.saves.last().cloned()), Some((5, input())));
        assert_eq!(mock::with(|ui| ui.back_count), 1);
    }
}

#[test]
fn invalid_web_password_stays_in_editor_and_valid_save_is_not_repeated() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::WebPassword);
        press(KEY_DONE);
        assert!(mock::with(|ui| ui.saves.is_empty()));
        for c in b"short" { press(*c as u32); }
        press(KEY_DONE);
        assert_eq!(mock::with(|ui| ui.back_count), 0);
        assert_eq!(input(), b"short");
        for c in b"123" { press(*c as u32); }
        press(KEY_DONE);
        press(KEY_DONE);
        assert_eq!(mock::with(|ui| ui.back_count), 1);
        assert_eq!(mock::with(|ui| ui.saves.clone()),
                   [(5, b"short".to_vec()), (5, b"short123".to_vec())]);
        open(core::ptr::null(), Completion::NshCommand);
        assert!(input().is_empty(), "password must never be saved as an NSH draft");
    }
}

#[test]
fn chinese_mode_replaces_the_number_row_with_a_fully_visible_candidate_bar() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let parent = setup_chinese(Completion::DeviceName);
        assert!(CHINESE);
        assert!(!UPPERCASE);
        assert_eq!(PINYIN_LEN, 0);
        assert_eq!(mock::with(|ui| ui.render_requests.clone()), [23]);
        assert!(action_codes(parent).contains(&KEY_CHINESE));
        assert_eq!(mock::object(INPUT_LABEL).text, b"|");

        // 视口里只有三行字母，候选条不在视口里。
        let viewport = viewport_of(parent);
        assert_eq!(mock::object(viewport).pos, (0, VIEWPORT_TOP_BAR));
        assert_eq!(mock::object(viewport).size, (VIEWPORT_WIDTH, VIEWPORT_HEIGHT_BAR));
        assert_eq!(mock::object(viewport).scroll_dir, fw_api::LV_DIR_HOR);
        let board = board_of(viewport);
        assert_eq!(mock::object(board).size, (BOARD_WIDTH, BOARD_HEIGHT_PINYIN));
        let keys = board_keys(board);
        assert_eq!(keys.len(), 26);
        assert!(!keys
            .iter()
            .any(|o| (KEY_CANDIDATE..=KEY_CANDIDATE_END).contains(&o.code.unwrap())));
        for &digit in &NUMBER_KEYS {
            assert!(!keys.iter().any(|k| k.code == Some(digit)));
        }
        // 候选条固定在页面上（parent 的直接子对象），键盘横滑时它不动。
        let bar = candidate_bar(parent);
        assert_eq!(bar.len(), CAND_BAR + 1);
        for (i, key) in bar[..CAND_BAR].iter().enumerate() {
            assert_eq!(key.code, Some(KEY_CANDIDATE + i as u32));
            assert_eq!(key.pos, (CAND_BAR_X + i as i32 * CAND_BAR_STEP, CAND_BAR_TOP));
            assert_eq!(key.size, (CAND_BAR_W, CAND_BAR_HEIGHT));
            assert!(key.pos.0 + key.size.0 <= VIEWPORT_WIDTH);
            assert_eq!(key.alignment, fw_api::align::TOP_LEFT);
        }
        let more = &bar[CAND_BAR];
        assert_eq!(more.code, Some(KEY_MORE));
        assert_eq!(more.pos, (CAND_BAR_X + CAND_BAR as i32 * CAND_BAR_STEP, CAND_BAR_TOP));
        assert_eq!(more.size, (CAND_MORE_W, CAND_BAR_HEIGHT));
        assert!(more.pos.0 + more.size.0 <= VIEWPORT_WIDTH);
        // 空拼音给全局高频字，候选条只活 6 个标签，每个都是完整 UTF-8 单字。
        assert_eq!(CAND_COUNT, 24);
        let common = pinyin_data::COMMON.as_bytes();
        for i in 0..CAND_BAR {
            assert_eq!(mock::object(CAND_LABELS[i]).text, common[i * 3..i * 3 + 3]);
            assert_eq!(CAND_CHARS[i][3], 0);
        }
    }
}

#[test]
fn pinyin_input_commits_whole_utf8_characters_and_saves_them() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceName);
        for c in b"ni" { press(*c as u32); }
        assert_eq!(PINYIN_LEN, 2);
        assert!(input().is_empty(), "拼音还没上屏时 TEXT 不能被污染");
        assert_eq!(mock::object(INPUT_LABEL).text, b"|ni");
        assert_eq!(&CAND_CHARS[0][..3], "你".as_bytes());
        assert_eq!(mock::object(CAND_LABELS[0]).text, "你".as_bytes());

        press(KEY_CANDIDATE);
        assert_eq!(input(), "你".as_bytes());
        assert_eq!(PINYIN_LEN, 0);
        assert_eq!(TEXT[LENGTH], 0);
        assert_eq!(mock::object(INPUT_LABEL).text, "你|".as_bytes());
        assert_eq!(mock::object(CAND_LABELS[0]).text, "了".as_bytes());

        for c in b"hao" { press(*c as u32); }
        assert_eq!(&CAND_CHARS[0][..3], "好".as_bytes());
        press(b' ' as u32); // 空格 = 上屏首选字
        assert_eq!(input(), "你好".as_bytes());
        assert!(std::str::from_utf8(&input()).is_ok());

        press(KEY_DONE);
        assert_eq!(
            mock::with(|ui| ui.saves.last().cloned()),
            Some((2, "你好".as_bytes().to_vec()))
        );
        assert_eq!(mock::with(|ui| ui.back_count), 1);
    }
}

#[test]
fn chinese_backspace_is_character_wise_and_capacity_is_byte_wise() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceModel);
        for _ in 0..10 {
            press(KEY_CANDIDATE); // 高频字首字「了」，每字 3 字节
        }
        assert_eq!(LENGTH, 30);
        assert_eq!(input(), "了".as_bytes().repeat(10));
        press(KEY_CANDIDATE); // 31 字节上限放不下第 11 个整字
        assert_eq!(LENGTH, 30);
        press(KEY_BACKSPACE);
        assert_eq!(LENGTH, 27);
        assert_eq!(input(), "了".as_bytes().repeat(9));
        assert_eq!(TEXT[27], 0);
        assert!(std::str::from_utf8(&input()).is_ok());

        // 拼音未上屏时退格先删拼音，不动已上屏文本。
        for c in b"zhong" { press(*c as u32); }
        let committed = input();
        press(KEY_BACKSPACE);
        assert_eq!(PINYIN_LEN, 4);
        assert_eq!(input(), committed);
        press(KEY_CLEAR);
        assert!(input().is_empty());
        assert_eq!(PINYIN_LEN, 0);
    }
}

#[test]
fn chinese_shift_switches_to_the_digit_page_and_discards_pending_pinyin() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let parent = setup_chinese(Completion::SystemText);
        for c in b"zh" { press(*c as u32); }
        assert_eq!(PINYIN_LEN, 2);
        press(KEY_SHIFT);
        assert!(UPPERCASE);
        assert_eq!(PINYIN_LEN, 0);
        assert!(render(parent));
        let board = board_of(viewport_of(parent));
        let codes = board_keys(board).iter().map(|o| o.code.unwrap()).collect::<Vec<_>>();
        for &digit in &NUMBER_KEYS { assert!(codes.contains(&digit), "missing digit"); }
        assert!(!codes.iter().any(|c| (KEY_CANDIDATE..=KEY_CANDIDATE_END).contains(c)));
        press(b'1' as u32);
        press(b'a' as u32);
        assert_eq!(input(), b"1A");

        press(KEY_CHINESE); // 退出中文，恢复英文页与默认大小写
        assert!(!CHINESE);
        assert!(UPPERCASE);
        assert!(input() == b"1A");
    }
}

#[test]
fn unknown_pinyin_falls_back_to_raw_letters_and_ok_commits_first_candidate() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::SystemText);
        press(b's' as u32);
        assert!(CAND_COUNT > 0, "单字母前缀也应有候选");
        press(b'h' as u32);
        assert!(CAND_COUNT > 0);
        for c in b"qx" { press(*c as u32); }
        assert_eq!(PINYIN_LEN, 4);
        assert_eq!(CAND_COUNT, 0);
        press(b' ' as u32);
        assert_eq!(input(), b"shqx", "无候选时拼音字母原样上屏");
        assert_eq!(PINYIN_LEN, 0);

        press(KEY_CLEAR);
        for c in b"de" { press(*c as u32); }
        press(KEY_DONE); // OK 前还有拼音，先上屏首选字再保存
        assert_eq!(input(), "的".as_bytes());
        assert_eq!(
            mock::with(|ui| ui.saves.last().cloned()),
            Some((1, "的".as_bytes().to_vec()))
        );
    }
}

#[test]
fn chinese_is_disabled_for_nsh_and_password_and_open_resets_the_mode() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (parent, _) = setup(Completion::NshCommand);
        assert!(!chinese_capable());
        assert!(!action_codes(parent).contains(&KEY_CHINESE));
        press(KEY_CHINESE);
        assert!(!CHINESE);
        assert!(mock::with(|ui| ui.render_requests.is_empty()));
        press(KEY_CANDIDATE);
        assert!(input().is_empty());

        let (parent, _) = setup(Completion::WebPassword);
        assert!(!action_codes(parent).contains(&KEY_CHINESE));
        press(KEY_CHINESE);
        assert!(!CHINESE);

        setup_chinese(Completion::DeviceName);
        for c in b"ni" { press(*c as u32); }
        assert!(CHINESE);
        open(core::ptr::null(), Completion::DeviceName);
        assert!(!CHINESE);
        assert!(UPPERCASE); // 回到英文页默认大写
        assert_eq!(PINYIN_LEN, 0);
        assert_eq!(CAND_LABELS, [0; MAX_CAND]);
        assert!(input().is_empty());
    }
}

// 候选按字频排序，不再按字母序跨音节混字。这是「展示逻辑很奇怪」的根因回归。
#[test]
fn candidates_are_frequency_ranked_and_do_not_mix_syllables() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceName);
        press(b's' as u32);
        // 字频首位是「是」，字母序混字会给出别的字。
        assert_eq!(&CAND_CHARS[0][..3], "是".as_bytes());
        assert_eq!(&CAND_CHARS[1][..3], "上".as_bytes());
        assert_eq!(&CAND_CHARS[2][..3], "说".as_bytes());

        press(KEY_CLEAR);
        for c in b"an" { press(*c as u32); }
        assert_eq!(&CAND_CHARS[0][..3], "安".as_bytes());
        // 「昂」属于 ang，旧逻辑按字母序把别的音节首字混进来；现在必须没有。
        let ang = "昂".as_bytes();
        for i in 0..CAND_COUNT {
            assert_ne!(&CAND_CHARS[i][..3], ang, "an 候选里混进了 ang 的字");
        }
    }
}

// 「更多」：候选比候选条多时打开纵向滚动的候选面板；点候选上屏并收回。
#[test]
fn more_opens_a_scrollable_panel_and_commit_collapses() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let parent = setup_chinese(Completion::DeviceName);
        for c in b"shi" { press(*c as u32); }
        assert!(CAND_COUNT > CAND_BAR);
        let expected_count = CAND_COUNT;
        press(KEY_MORE);
        assert!(BROWSING);
        assert_eq!(mock::with(|ui| ui.render_requests.clone()), [23, 23]);
        assert!(render(parent));

        let viewport = viewport_of(parent);
        assert_eq!(mock::object(viewport).pos, (0, VIEWPORT_TOP_BAR));
        assert_eq!(mock::object(viewport).scroll_dir, fw_api::LV_DIR_VER);
        // 面板态候选条仍然固定在视口上方，不随面板滚动。
        let bar = candidate_bar(parent);
        assert_eq!(bar.len(), CAND_BAR + 1);
        assert_eq!(bar[0].pos, (CAND_BAR_X, CAND_BAR_TOP));
        let content = panel_content_of(viewport);
        let rows = (expected_count + GRID_COLS - 1) / GRID_COLS;
        assert_eq!(
            mock::object(content).size,
            (VIEWPORT_WIDTH, (rows as i32) * GRID_ROW_STEP)
        );
        assert!(
            mock::object(content).size.1 > VIEWPORT_HEIGHT_BAR,
            "候选多到超出视口时必须能纵向滚动"
        );
        let cells = panel_cells(content);
        assert_eq!(cells.len(), expected_count);
        assert!(cells.iter().all(|o| (KEY_CANDIDATE..=KEY_CANDIDATE_END).contains(&o.code.unwrap())));
        for (i, cell) in cells.iter().enumerate() {
            assert_eq!(cell.pos.0, GRID_X + (i % GRID_COLS) as i32 * GRID_STEP_X);
            assert_eq!(cell.pos.1, (i / GRID_COLS) as i32 * GRID_ROW_STEP);
            assert_eq!(cell.size, (GRID_W, KEY_HEIGHT));
            assert!(cell.pos.0 + cell.size.0 <= VIEWPORT_WIDTH);
        }
        // 操作区第一键换成「收起」。
        let actions = action_codes(parent);
        assert_eq!(actions[0], KEY_COLLAPSE);

        // 上屏第 11 个候选（提交会收回面板并清空拼音，先记下期望字）。
        let expected = CAND_CHARS[10][..3].to_vec();
        press(KEY_CANDIDATE + 10);
        assert!(!BROWSING);
        assert_eq!(input(), expected);
        assert_eq!(PINYIN_LEN, 0);
    }
}

// 候选不超过候选条容量时，「更多」不动作（没有更多可看）。
#[test]
fn more_is_inert_when_candidates_fit_the_bar() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceName);
        for c in b"de" { press(*c as u32); }
        assert_eq!(CAND_COUNT, 4);
        let before = mock::with(|ui| ui.render_requests.clone());
        press(KEY_MORE);
        assert!(!BROWSING);
        assert_eq!(mock::with(|ui| ui.render_requests.clone()), before);
    }
}

// 收起：回到字母键盘，拼音与已上屏文本都保留。
#[test]
fn collapse_returns_to_the_letter_keyboard_preserving_pinyin() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let parent = setup_chinese(Completion::DeviceName);
        for c in b"ni" { press(*c as u32); }
        press(KEY_MORE);
        assert!(BROWSING);
        assert!(render(parent));
        assert_eq!(
            panel_cells(panel_content_of(viewport_of(parent))).len(),
            CAND_COUNT
        );
        press(KEY_COLLAPSE);
        assert!(!BROWSING);
        assert!(render(parent));
        let keys = board_keys(board_of(viewport_of(parent)));
        assert_eq!(keys.len(), 26);
        assert_eq!(candidate_bar(parent).len(), CAND_BAR + 1);
        assert_eq!(action_codes(parent)[0], KEY_SHIFT);
        assert_eq!(PINYIN_LEN, 2);
        assert!(input().is_empty());
        assert_eq!(&CAND_CHARS[0][..3], "你".as_bytes());
    }
}

// 面板态退格：拼音变短，候选面板按新候选数重建。
#[test]
fn browsing_backspace_rebuilds_panel_with_current_candidates() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let parent = setup_chinese(Completion::DeviceName);
        for c in b"shi" { press(*c as u32); }
        let before = CAND_COUNT;
        press(KEY_MORE);
        assert!(render(parent));
        assert_eq!(
            panel_cells(panel_content_of(viewport_of(parent))).len(),
            before
        );
        press(KEY_BACKSPACE); // shi -> sh
        assert_eq!(PINYIN_LEN, 2);
        assert!(BROWSING);
        assert!(render(parent));
        let viewport = viewport_of(parent);
        assert_eq!(mock::object(viewport).scroll_dir, fw_api::LV_DIR_VER);
        let cells = panel_cells(panel_content_of(viewport));
        assert_eq!(cells.len(), CAND_COUNT);
        assert_eq!(CAND_PREFIX, 2);
    }
}

// 连续输入多个音节：候选始终给最长的完整音节，上屏后剩下的拼音继续给候选。
#[test]
fn multi_syllable_input_advances_one_syllable_per_space() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceName);
        for c in b"nihao" { press(*c as u32); }
        assert_eq!(PINYIN_LEN, 5);
        assert_eq!(CAND_PREFIX, 2);
        assert_eq!(&CAND_CHARS[0][..3], "你".as_bytes());
        assert_eq!(mock::object(INPUT_LABEL).text, b"|nihao");

        press(b' ' as u32); // 上屏「你」，剩下 hao
        assert_eq!(input(), "你".as_bytes());
        assert_eq!(PINYIN_LEN, 3);
        assert_eq!(&PINYIN[..3], &b"hao"[..]);
        assert_eq!(CAND_PREFIX, 3);
        assert_eq!(&CAND_CHARS[0][..3], "好".as_bytes());
        assert_eq!(mock::object(INPUT_LABEL).text, "你|hao".as_bytes());

        press(b' ' as u32);
        assert_eq!(input(), "你好".as_bytes());
        assert_eq!(PINYIN_LEN, 0);
        assert_eq!(CAND_PREFIX, 0);
        assert_eq!(mock::object(INPUT_LABEL).text, "你好|".as_bytes());
    }
}

// 完整音节后面挂着半截字母时候选不消失（取完整的 ni）；完全查不到的尾巴仍是 0。
#[test]
fn partial_tail_after_complete_syllable_keeps_candidates() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceName);
        for c in b"nih" { press(*c as u32); }
        assert_eq!(PINYIN_LEN, 3);
        assert_eq!(CAND_PREFIX, 2);
        assert_eq!(&CAND_CHARS[0][..3], "你".as_bytes());

        press(KEY_CLEAR);
        for c in b"shqx" { press(*c as u32); }
        assert_eq!(CAND_COUNT, 0);
        assert_eq!(CAND_PREFIX, 0);
    }
}

// OK 一次把所有挂起的音节上屏；查不到候选的尾巴原样拼在后面。
#[test]
fn ok_commits_all_pending_syllables_and_keeps_unknown_tail() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::SystemText);
        for c in b"nishqx" { press(*c as u32); }
        assert_eq!(CAND_PREFIX, 2);
        press(KEY_DONE);
        assert_eq!(input(), "你shqx".as_bytes());
        assert_eq!(PINYIN_LEN, 0);
        assert_eq!(
            mock::with(|ui| ui.saves.last().cloned()),
            Some((1, "你shqx".as_bytes().to_vec()))
        );
        assert_eq!(mock::with(|ui| ui.back_count), 1);
    }
}

// 拼音还挂着时按标点：先把音节上屏，标点落在汉字后面。
#[test]
fn punctuation_after_pending_pinyin_goes_after_the_character() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceName);
        for c in b"ni" { press(*c as u32); }
        press(b'.' as u32);
        assert_eq!(input(), "你.".as_bytes());
        assert_eq!(PINYIN_LEN, 0);
        press(KEY_DONE);
        assert_eq!(
            mock::with(|ui| ui.saves.last().cloned()),
            Some((2, "你.".as_bytes().to_vec()))
        );
    }
}

// 多音节全部由候选组成时，OK 一次上屏全部，不是只上屏第一个音节。
#[test]
fn ok_commits_every_multi_syllable_candidate() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup_chinese(Completion::DeviceName);
        for c in b"nihao" { press(*c as u32); }
        press(KEY_DONE);
        assert_eq!(input(), "你好".as_bytes());
        assert_eq!(
            mock::with(|ui| ui.saves.last().cloned()),
            Some((2, "你好".as_bytes().to_vec()))
        );
    }
}

// 词表完整性：偏移连续、段长对齐 3 字节、每个候选都是完整 UTF-8 单字、同段无重复。
#[test]
fn pinyin_table_is_well_formed() {
    assert_eq!(pinyin_data::PREFIX_OFF.len(), pinyin_data::COUNT + 1);
    assert_eq!(pinyin_data::CANDIDATE_OFF.len(), pinyin_data::COUNT + 1);
    assert_eq!(pinyin_data::COMPLETE.len(), pinyin_data::COUNT);
    assert_eq!(pinyin_data::PREFIX_OFF[0], 0);
    assert_eq!(pinyin_data::CANDIDATE_OFF[0], 0);
    assert_eq!(
        *pinyin_data::PREFIX_OFF.last().unwrap() as usize,
        pinyin_data::PREFIXES.len()
    );
    assert_eq!(
        *pinyin_data::CANDIDATE_OFF.last().unwrap() as usize,
        pinyin_data::CANDIDATES.len()
    );
    let mut prev = 0usize;
    for i in 0..pinyin_data::COUNT {
        let (a, b) = (
            pinyin_data::PREFIX_OFF[i] as usize,
            pinyin_data::PREFIX_OFF[i + 1] as usize,
        );
        assert!(a == prev && a < b, "prefix offsets must be contiguous");
        prev = b;
        let prefix = prefix_bytes(i);
        assert!(prefix.iter().all(|c| c.is_ascii_lowercase()));
        let (ca, cb) = (
            pinyin_data::CANDIDATE_OFF[i] as usize,
            pinyin_data::CANDIDATE_OFF[i + 1] as usize,
        );
        let bytes = candidate_bytes(i);
        assert_eq!(cb - ca, bytes.len());
        let text = std::str::from_utf8(bytes).expect("candidate blob must be utf-8");
        let mut seen = std::collections::BTreeSet::new();
        for ch in text.chars() {
            assert_eq!(ch.len_utf8(), 3, "candidate must be a 3-byte BMP char");
            assert!(seen.insert(ch), "duplicate candidate in one prefix");
        }
        if pinyin_data::COMPLETE[i] != 0 {
            assert!(
                prefix.len() >= 2,
                "single-letter syllables are prefix unions, not complete"
            );
        }
    }
}
