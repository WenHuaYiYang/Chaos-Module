use super::*;
use crate::mock;

unsafe fn setup(completion: Completion) -> (u32, u32, u32) {
    mock::reset();
    open(core::ptr::null(), completion);
    let parent = fw_api::obj_create(0, 0);
    fw_api::obj_set_size(parent, 336, 424);
    assert!(render(parent));
    let viewport = mock::with(|ui| {
        ui.objects
            .iter()
            .position(|o| o.parent == parent && o.size == (VIEWPORT_WIDTH, VIEWPORT_HEIGHT))
            .unwrap() as u32
            + 1
    });
    let board = mock::with(|ui| {
        ui.objects
            .iter()
            .position(|o| o.parent == viewport)
            .unwrap() as u32
            + 1
    });
    (parent, viewport, board)
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
            // Raw 9 is deliberate: firmware 0x79360 dispatches 9 to center/center,
            // while the old value 8 selects right/center. Do not compare to a copied mock constant.
            assert_eq!(
                labels[0].alignment, 9,
                "key {:?} must be centered",
                key.code
            );
            assert_eq!(labels[0].pos, (0, 0));
            count += 1;
        }
        assert_eq!(count, 42); // 36 characters + ABC/SPACE/backspace/CLR/period/OK
    });
}

#[test]
fn every_key_label_uses_firmware_center_alignment() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(Completion::DeviceName);
        assert_key_labels_centered();
        // Only key captions change; the editable input stays anchored at its top-left.
        assert_eq!(mock::object(INPUT_LABEL).alignment, fw_api::align::TOP_LEFT);
        assert_eq!(mock::object(INPUT_LABEL).pos, (8, 4));
    }
}

#[test]
fn full_qwerty_rows_are_ordered_staggered_and_reachable() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (_, _, board) = setup(Completion::DeviceName);
        let keys = mock::with(|ui| {
            ui.objects
                .iter()
                .filter(|o| o.parent == board)
                .cloned()
                .collect::<Vec<_>>()
        });
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
                row_keys
                    .iter()
                    .map(|k| k.code.unwrap() as u8)
                    .collect::<Vec<_>>(),
                *expected
            );
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
            assert!(key.pos.1 >= 0 && key.pos.1 + key.size.1 <= BOARD_HEIGHT);
            let offset = (key.pos.0 + key.size.0 - VIEWPORT_WIDTH).max(0);
            assert!(offset <= BOARD_WIDTH - VIEWPORT_WIDTH);
            assert!(key.pos.0 - offset >= 0);
        }
    }
}

#[test]
fn only_the_keyboard_scrolls_and_actions_fit_above_the_gesture_area() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (parent, viewport, board) = setup(Completion::SystemVersion);
        let view = mock::object(viewport);
        assert_eq!(view.scroll_dir, fw_api::LV_DIR_HOR);
        assert_ne!(view.flags & fw_api::LV_OBJ_FLAG_SCROLLABLE, 0);
        assert_eq!(
            view.flags
                & (fw_api::LV_OBJ_FLAG_SCROLL_CHAIN
                    | fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE
                    | fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC),
            0
        );
        assert_eq!(
            mock::object(parent).flags & fw_api::LV_OBJ_FLAG_SCROLLABLE,
            0
        );
        assert_eq!(
            mock::object(board).flags & fw_api::LV_OBJ_FLAG_SCROLLABLE,
            0
        );
        assert_eq!(mock::object(INPUT_LABEL).parent, parent);
        let actions = mock::with(|ui| {
            ui.objects
                .iter()
                .filter(|o| o.parent == parent && o.code.is_some())
                .cloned()
                .collect::<Vec<_>>()
        });
        assert_eq!(actions.len(), 6);
        assert_eq!(
            actions.iter().map(|o| o.code.unwrap()).collect::<Vec<_>>(),
            [
                KEY_SHIFT,
                b' ' as u32,
                KEY_BACKSPACE,
                KEY_CLEAR,
                b'.' as u32,
                KEY_DONE
            ]
        );
        for action in &actions {
            assert!(action.pos.0 >= 0 && action.pos.0 + action.size.0 <= 336);
            assert!(action.pos.1 >= VIEWPORT_TOP + VIEWPORT_HEIGHT);
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
fn shift_updates_every_letter_without_rebuilding_or_losing_the_scroll_position() {
    let _lock = mock::TEST_LOCK.lock().unwrap();
    unsafe {
        let (_, viewport, _) = setup(Completion::DeviceName);
        mock::edit(viewport, |o| o.scroll_x = BOARD_WIDTH - VIEWPORT_WIDTH);
        press(b'q' as u32);
        let count = mock::with(|ui| ui.objects.len());
        press(KEY_SHIFT);
        assert_eq!(mock::with(|ui| ui.objects.len()), count);
        assert_eq!(
            mock::object(viewport).scroll_x,
            BOARD_WIDTH - VIEWPORT_WIDTH
        );
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
        let (parent, _, _) = setup(Completion::DeviceName);
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
