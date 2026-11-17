use super::*;
use crate::{MOCK, Mock};

unsafe fn setup() {
    MOCK.with(|m|*m.borrow_mut()=Mock {awake:true,..Mock::default()});
    VALUE.store(0,Ordering::Release); ROOT.store(0,Ordering::Release);
    STATUS.store(WAITING,Ordering::Release); SAVE_FAILED.store(false,Ordering::Release);
    EPOCH.store(0,Ordering::Release);
    RUNTIME=Runtime::new(); crate::state::SIM_LITE=0;
}
unsafe fn start() {setup();toggle();tick();assert_ne!(RUNTIME.input,0);}
unsafe fn poll(now:u32,packet:bool)->fw::GyroInputData {
    MOCK.with(|m|{let mut m=m.borrow_mut();m.now=now;
        if packet {let mut b=vec![0u8;24];b[..8].copy_from_slice(&(now as u64*1000+1).to_le_bytes());m.packets.push_back(b);}
    });
    let mut data=fw::GyroInputData::default();read_input(RUNTIME.input,&mut data);data
}
unsafe fn poll_moving(now:u32)->fw::GyroInputData {
    MOCK.with(|m|{
        let mut m=m.borrow_mut();let mut b=vec![0u8;24];
        b[..8].copy_from_slice(&(now as u64*1000+1).to_le_bytes());
        b[8..12].copy_from_slice(&0.7f32.to_le_bytes());m.packets.push_back(b);
    });
    poll(now,false)
}
unsafe fn press()->u32 {
    for i in 0..150 {let now=i*20;if poll(now,true).state==1{return now;}}
    panic!("no press");
}

#[test]
fn disabled_by_default_and_invalid_config_does_not_allocate() {unsafe {
    setup();assert!(!enabled());tick();
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.creates,0);assert_eq!(m.sensor_opens,0);});
    for bytes in [b"GTP1\x02".to_vec(),b"BAD!\x01".to_vec(),b"GTP1\x01junk".to_vec()] {
        setup();MOCK.with(|m|m.borrow_mut().files.insert(String::from_utf8_lossy(&CONFIG[..CONFIG.len()-1]).into(),bytes));
        assert!(!enabled());tick();assert_eq!(RUNTIME.input,0);
    }
}}

#[test]
fn persistent_switch_restarts_without_creating_from_event_callback() {unsafe {
    setup();toggle();assert!(enabled());assert_eq!(RUNTIME.input,0);
    MOCK.with(|m|assert_eq!(Config::decode(&m.borrow().files["/data/chaos/gyro-pointer.cfg"]),Some(Config{enabled:true,..Config::defaults()})));
    VALUE.store(0,Ordering::Release);assert!(enabled());tick();tick();
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.creates,1);assert_eq!(m.overlay_creates,1);});
}}

#[test]
fn partial_writes_are_completed_before_atomic_rename() {unsafe {
    setup();MOCK.with(|m|m.borrow_mut().write_limit=2);toggle();
    assert!(!SAVE_FAILED.load(Ordering::Acquire));
    MOCK.with(|m|{let m=m.borrow();assert_eq!(Config::decode(&m.files["/data/chaos/gyro-pointer.cfg"]),Some(Config{enabled:true,..Config::defaults()}));assert!(!m.files.contains_key("/data/chaos/gyro-pointer.cfg.tmp"));});
}}

#[test]
fn save_failure_keeps_old_file_and_off_gate_still_wins() {unsafe {
    start();poll(0,true);MOCK.with(|m|m.borrow_mut().rename_failed=true);
    toggle();assert!(!enabled());assert!(SAVE_FAILED.load(Ordering::Acquire));tick();
    assert_eq!(RUNTIME.input,0);
    MOCK.with(|m|{let m=m.borrow();assert!(Config::decode(&m.files["/data/chaos/gyro-pointer.cfg"]).unwrap().enabled);assert_eq!(m.deletes,1);assert_eq!(m.sensor_closes,1);assert!(!m.files.contains_key("/data/chaos/gyro-pointer.cfg.tmp"));});
}}

#[test]
fn disable_during_a_press_cancels_before_release_and_defers_retirement() {unsafe {
    start();let now=press();toggle();
    MOCK.with(|m|assert_eq!(m.borrow().deletes,0));
    let output=poll(now+20,false);assert_eq!(output.state,0);
    MOCK.with(|m|{let m=m.borrow();assert!(m.resets>0);assert_eq!(m.deletes,0);assert!(!m.visible);});
    tick();tick();assert_eq!(RUNTIME.input,0);
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.deletes,1);assert_eq!(m.overlay_deletes,1);});
}}

#[test]
fn screen_off_closes_sensor_and_wake_recalibrates() {unsafe {
    start();let now=press();MOCK.with(|m|m.borrow_mut().awake=false);
    assert_eq!(poll(now+20,false).state,0);
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.sensor_closes,1);assert_eq!(m.period,1000);assert!(!m.visible);});
    MOCK.with(|m|m.borrow_mut().awake=true);
    assert_eq!(poll(now+40,true).state,0);
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.sensor_opens,2);assert_eq!(m.period,20);assert!(!m.cursor.3);});
}}

#[test]
fn real_touch_aborts_the_virtual_press() {unsafe {
    start();let now=press();MOCK.with(|m|m.borrow_mut().physical=true);
    let resets=MOCK.with(|m|m.borrow().resets);
    assert_eq!(poll(now+20,true).state,0);
    MOCK.with(|m|assert!(m.borrow().resets>resets));
}}

#[test]
fn rapid_off_on_does_not_resume_a_previously_armed_press() {unsafe {
    start();let now=press();let resets=MOCK.with(|m|m.borrow().resets);
    toggle();toggle();assert!(enabled());
    assert_eq!(poll(now+20,true).state,0);
    MOCK.with(|m|{let m=m.borrow();assert!(m.resets>resets);assert!(!m.cursor.3);assert_eq!(m.deletes,0);});
}}

#[test]
fn unavailable_sensor_retries_with_backoff_and_never_presses() {unsafe {
    start();MOCK.with(|m|m.borrow_mut().sensor_failed=true);
    for i in 0..90 {assert_eq!(poll(i*20,false).state,0);}
    MOCK.with(|m|assert_eq!(m.borrow().sensor_opens,1));
    poll(2020,false);MOCK.with(|m|assert_eq!(m.borrow().sensor_opens,2));
    assert_eq!(STATUS.load(Ordering::Acquire),SENSOR_FAILED);
}}

#[test]
fn stalled_stream_is_reopened_without_a_phantom_click() {unsafe {
    start();poll(0,true);
    for i in 1..110 {assert_eq!(poll(i*20,false).state,0);}
    MOCK.with(|m|assert_eq!(m.borrow().sensor_closes,1));
    assert_eq!(RUNTIME.sensor,-1);
}}

#[test]
fn malformed_packet_cancels_an_active_press() {unsafe {
    start();let now=press();MOCK.with(|m|m.borrow_mut().packets.push_back(vec![0;8]));
    assert_eq!(poll(now+20,false).state,0);
    MOCK.with(|m|{let m=m.borrow();assert!(m.resets>0);assert_eq!(m.sensor_closes,1);});
}}

#[test]
fn backlog_is_bounded_and_not_used_to_finish_a_dwell() {unsafe {
    start();let now=press();
    MOCK.with(|m|{let mut m=m.borrow_mut();for i in 0..20 {let mut b=vec![0u8;24];b[..8].copy_from_slice(&((now as u64+i+1)*1000).to_le_bytes());m.packets.push_back(b);}});
    let reads=MOCK.with(|m|m.borrow().sensor_reads);
    assert_eq!(poll(now+20,false).state,0);
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.sensor_reads-reads,8);assert_eq!(m.packets.len(),12);assert!(m.resets>0);});
}}

#[test]
fn removed_overlay_is_rebuilt_before_accepting_more_input() {unsafe {
    start();let now=press();ROOT.store(0,Ordering::Release);
    assert_eq!(poll(now+20,true).state,0);tick();
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.overlay_creates,2);assert_eq!(m.creates,1);});
    assert_ne!(ROOT.load(Ordering::Acquire),0);
}}

#[test]
fn simulator_mode_stops_the_feature() {unsafe {
    start();crate::state::SIM_LITE=1;tick();assert_eq!(RUNTIME.input,0);
    MOCK.with(|m|assert_eq!(m.borrow().deletes,1));
}}

#[test]
fn packed_native_data_layout_is_not_the_host_enum_layout() {
    assert_eq!(core::mem::size_of::<fw::GyroInputData>(),20);
    assert_eq!(core::mem::offset_of!(fw::GyroInputData,state),18);
    assert_eq!(core::mem::offset_of!(fw::GyroInputData,continue_reading),19);
}

#[test]
fn the_runtime_creates_the_enlarged_dot_and_ring() {unsafe {
    start();MOCK.with(|m|assert_eq!(m.borrow().overlay_size,(48,20)));
}}

#[test]
fn upgrading_keeps_the_old_enable_flag_and_supplies_parameter_defaults() {unsafe {
    setup();MOCK.with(|m|m.borrow_mut().files.insert("/data/chaos/gyro-pointer.cfg".into(),b"GTP1\x01".to_vec()));
    assert!(enabled());assert_eq!(sensitivity(),100);assert_eq!(confirm_ms(),900);tick();
    assert_ne!(RUNTIME.input,0);
}}

#[test]
fn saved_preferences_survive_reload_and_switch_changes() {unsafe {
    start();assert!(set_parameter(0,b"175"));assert!(set_parameter(1,b"1500"));assert!(set_parameter(2,b"20"));
    assert!(set_parameter(3,b"7"));
    toggle();assert!(!enabled());toggle();assert!(enabled());
    VALUE.store(0,Ordering::Release);SENSITIVITY.store(100,Ordering::Release);CONFIRM_MS.store(900,Ordering::Release);
    TOLERANCE_PX.store(6,Ordering::Release);
    PAGE_PAUSE_S.store(2,Ordering::Release);
    assert!(enabled());assert_eq!(sensitivity(),175);assert_eq!(confirm_ms(),1500);assert_eq!(tolerance_px(),20);
    assert_eq!(page_pause_s(),7);
}}

#[test]
fn failed_preference_save_preserves_live_values_and_old_file() {unsafe {
    start();assert!(set_parameter(0,b"125"));
    MOCK.with(|m|m.borrow_mut().rename_failed=true);
    assert!(!set_parameter(0,b"250"));assert!(save_failed());assert_eq!(sensitivity(),125);
    MOCK.with(|m|assert_eq!(Config::decode(&m.borrow().files["/data/chaos/gyro-pointer.cfg"]).unwrap().sensitivity,125));
}}

#[test]
fn invalid_preference_values_do_not_change_live_settings_or_disk() {unsafe {
    start();for (index,bad) in [(0,b"24".as_slice()),(0,b"301"),(1,b"299"),(1,b"5001"),(0,b""),(1,b"-1"),(2,b"0"),(2,b"31"),(3,b"11"),(3,b"-1"),(3,b"1.5")] {
        assert!(!set_parameter(index,bad));assert_eq!(sensitivity(),100);assert_eq!(confirm_ms(),900);assert_eq!(tolerance_px(),6);
        assert_eq!(page_pause_s(),2);
    }
}}

#[test]
fn saving_preferences_cancels_an_old_press_and_uses_the_new_delay() {unsafe {
    start();let now=press();let resets=MOCK.with(|m|m.borrow().resets);
    assert!(set_parameter(1,b"1800"));assert_eq!(poll(now+20,true).state,0);
    MOCK.with(|m|assert!(m.borrow().resets>resets));
    for i in 2..80 {assert_eq!(poll(now+i*20,true).state,0);}
}}

#[test]
fn gtp2_upgrade_keeps_old_parameters_and_saves_new_tolerance() {unsafe {
    setup();let mut old=vec![b'G',b'T',b'P',b'2',1,175,0,0x40,0x06,0];
    old[9]=old[..9].iter().fold(0xa5,|a,b|a^b);
    MOCK.with(|m|m.borrow_mut().files.insert("/data/chaos/gyro-pointer.cfg".into(),old));
    assert!(enabled());assert_eq!(sensitivity(),175);assert_eq!(confirm_ms(),1600);assert_eq!(tolerance_px(),6);
    assert!(set_parameter(2,b"12"));VALUE.store(0,Ordering::Release);
    assert!(enabled());assert_eq!(sensitivity(),175);assert_eq!(confirm_ms(),1600);assert_eq!(tolerance_px(),12);
}}

#[test]
fn failed_tolerance_save_keeps_the_old_value_and_file() {unsafe {
    start();assert!(set_parameter(2,b"9"));MOCK.with(|m|m.borrow_mut().rename_failed=true);
    assert!(!set_parameter(2,b"20"));assert!(save_failed());assert_eq!(tolerance_px(),9);
    MOCK.with(|m|assert_eq!(Config::decode(&m.borrow().files["/data/chaos/gyro-pointer.cfg"]).unwrap().tolerance_px,9));
}}

#[test]
fn saving_tolerance_cancels_an_in_progress_click() {unsafe {
    start();let now=press();let resets=MOCK.with(|m|m.borrow().resets);
    assert!(set_parameter(2,b"20"));assert_eq!(poll(now+20,true).state,0);
    MOCK.with(|m|assert!(m.borrow().resets>resets));
}}

#[test]
fn page_switch_cancels_old_input_and_resumes_in_two_seconds_without_stillness() {unsafe {
    start();let now=press();let resets=MOCK.with(|m|m.borrow().resets);
    MOCK.with(|m|m.borrow_mut().page=[101,0x0016_0000,200,0]);
    assert_eq!(poll_moving(now+20).state,0);
    MOCK.with(|m|{let m=m.borrow();assert!(m.resets>resets);assert_eq!(m.cursor,(168,240,0,false));});
    for i in 1..100 {
        assert_eq!(poll_moving(now+20+i*20).state,0);
        MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,false)));
    }
    assert_eq!(poll_moving(now+2020).state,0);
    MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,true)));
    assert_eq!(poll_moving(now+2040).state,0);
    MOCK.with(|m|assert!(m.borrow().cursor.1>240));
}}

#[test]
fn page_rebuild_and_home_launcher_transition_each_restart_the_fixed_pause() {unsafe {
    start();let mut now=press();
    let identities=[[10,0x0010_0000,20,1],[10,0x0010_0000,20,0],[10,0x0010_0000,24,0],
                    [12,0x0016_0000,24,0],[12,0x0016_0001,24,0]];
    for page in identities {
        MOCK.with(|m|m.borrow_mut().page=page);
        now+=20;assert_eq!(poll(now,true).state,0);
        for _ in 0..60 {now+=20;assert_eq!(poll_moving(now).state,0);}
        MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,false)));
    }
    for _ in 0..40 {now+=20;assert_eq!(poll_moving(now).state,0);}
    MOCK.with(|m|assert!(m.borrow().cursor.3));
}}

#[test]
fn real_touch_during_the_page_pause_keeps_priority_without_restarting_the_timer() {unsafe {
    start();let now=press();MOCK.with(|m|{let mut m=m.borrow_mut();m.page=[11,0x0016_0000,24,0];m.physical=true;});
    for i in 1..120 {assert_eq!(poll_moving(now+i*20).state,0);}
    MOCK.with(|m|m.borrow_mut().physical=false);
    assert_eq!(poll_moving(now+2400).state,0);
    MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,true)));
}}

#[test]
fn page_switch_does_not_replace_the_input_or_reopen_a_healthy_sensor() {unsafe {
    start();let now=press();
    MOCK.with(|m|m.borrow_mut().page=[101,0x0016_0000,200,0]);
    for i in 1..130 {poll_moving(now+i*20);}
    MOCK.with(|m|{let m=m.borrow();assert_eq!(m.creates,1);assert_eq!(m.sensor_opens,1);assert_eq!(m.sensor_closes,0);assert_eq!(m.deletes,0);});
}}

#[test]
fn page_switch_during_backlog_still_waits_two_seconds_after_the_stream_catches_up() {unsafe {
    start();let now=press();
    MOCK.with(|m|{
        let mut m=m.borrow_mut();m.page=[101,0x0016_0000,200,0];
        for i in 0..9 {let mut b=vec![0u8;24];b[..8].copy_from_slice(&((now as u64+i+1)*1000).to_le_bytes());m.packets.push_back(b);}
    });
    assert_eq!(poll(now+20,false).state,0);
    assert_eq!(poll_moving(now+40).state,0);
    for i in 1..100 {
        assert_eq!(poll_moving(now+40+i*20).state,0);
        MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,false)));
    }
    assert_eq!(poll_moving(now+2040).state,0);
    MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,true)));
}}

#[test]
fn saved_pause_controls_the_actual_page_transition_delay_including_zero() {unsafe {
    for seconds in [0,1,5,10] {
        start();assert!(set_parameter(3,seconds.to_string().as_bytes()));
        let now=press();MOCK.with(|m|m.borrow_mut().page=[101,0x0016_0000,200,0]);
        assert_eq!(poll_moving(now+20).state,0);
        if seconds>0 {
            for i in 1..seconds*50 {
                assert_eq!(poll_moving(now+20+i*20).state,0);
                MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,false)));
            }
            assert_eq!(poll_moving(now+20+seconds*1000).state,0);
        }
        MOCK.with(|m|assert_eq!(m.borrow().cursor,(168,240,0,true)));
    }
}}

#[test]
fn failed_pause_save_preserves_previous_value_and_cancelling_it_never_clicks() {unsafe {
    start();assert!(set_parameter(3,b"4"));let now=press();
    let before=MOCK.with(|m|m.borrow().files["/data/chaos/gyro-pointer.cfg"].clone());
    MOCK.with(|m|m.borrow_mut().rename_failed=true);
    assert!(!set_parameter(3,b"8"));assert!(save_failed());assert_eq!(page_pause_s(),4);
    MOCK.with(|m|assert_eq!(m.borrow().files["/data/chaos/gyro-pointer.cfg"],before));
    MOCK.with(|m|m.borrow_mut().rename_failed=false);
    assert!(set_parameter(3,b"0"));assert_eq!(poll(now+20,true).state,0);
    MOCK.with(|m|assert!(m.borrow().resets>0));
}}

#[test]
fn gtp3_upgrade_keeps_all_previous_values_when_pause_is_saved() {unsafe {
    setup();let mut old=vec![b'G',b'T',b'P',b'3',1,175,0,0x40,0x06,20,0,0];
    old[11]=old[..11].iter().fold(0xa5,|a,b|a^b);
    MOCK.with(|m|m.borrow_mut().files.insert("/data/chaos/gyro-pointer.cfg".into(),old));
    assert!(enabled());assert_eq!(page_pause_s(),2);assert_eq!(tolerance_px(),20);
    assert!(set_parameter(3,b"6"));VALUE.store(0,Ordering::Release);
    assert!(enabled());assert_eq!(sensitivity(),175);assert_eq!(confirm_ms(),1600);
    assert_eq!(tolerance_px(),20);assert_eq!(page_pause_s(),6);
    MOCK.with(|m|{let m=m.borrow();let bytes=&m.files["/data/chaos/gyro-pointer.cfg"];assert_eq!(bytes.len(),14);assert_eq!(&bytes[..4],b"GTP4");});
}}
