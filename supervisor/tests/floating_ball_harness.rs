#![allow(dead_code, static_mut_refs)]
use std::{cell::RefCell, collections::BTreeMap};
#[path = "../src/floating_ball.rs"]
mod floating_ball;
#[path = "floating_ball_fw.rs"]
mod fw_api;
mod mem {
    pub fn safe_ptr(p: u32) -> bool {
        p != 0
    }
}
mod page {
    pub enum Pid {
        FloatingBall = 58,
    }
}
fn render_req(_: u32) {}

#[derive(Clone, Default)]
struct Object {
    parent: u32,
    flags: u32,
    size: (u32, u32),
    position: (i32, i32),
    text: String,
    events: Vec<(u32, u32)>,
    properties: BTreeMap<u32, u32>,
}
struct World {
    image_publications: u32,
    gpu_idle: bool,
    now: u32,
    on: bool,
    timeout: u32,
    brightness: u32,
    brightness_writes: u32,
    battery: Option<u8>,
    battery_reads: u32,
    muted: bool,
    home_requests: u32,
    home_available: bool,
    home_depth: u32,
    home_app: u32,
    home_watchface: bool,
    home_step: bool,
    flipped: bool,
    recording: u8,
    auto: u32,
    timeout_refreshes: u32,
    objects: BTreeMap<u32, Object>,
    next_object: u32,
    deleted: Vec<u32>,
    timers: BTreeMap<u32, u32>,
    files: BTreeMap<String, Vec<u8>>,
    handles: BTreeMap<i32, (String, usize)>,
    next_fd: i32,
    read_limit: usize,
    write_limit: usize,
    write_budget: usize,
    rename_error: i32,
    close_error: i32,
    writes: u32,
    reads: u32,
    clock_reads: u32,
    allocations: u32,
    input: fw_api::floating_ball::Input,
    shot_state: u8,
    shot_requests: u32,
    shot_error: i32,
}
impl Default for World {
    fn default() -> Self {
        let mut objects = BTreeMap::new();
        objects.insert(1, Object::default());
        Self {
            image_publications: 0,
            gpu_idle: true,
            now: 1000,
            on: true,
            timeout: 15_000,
            brightness: 600,
            brightness_writes: 0,
            battery: Some(75),
            battery_reads: 0,
            muted: false,
            home_requests: 0,
            home_available: true,
            home_depth: 1,
            home_app: 16,
            home_watchface: true,
            home_step: false,
            flipped: false,
            recording: 0,
            auto: 1,
            timeout_refreshes: 0,
            objects,
            next_object: 10,
            deleted: Vec::new(),
            timers: BTreeMap::new(),
            files: BTreeMap::new(),
            handles: BTreeMap::new(),
            next_fd: 3,
            read_limit: 3,
            write_limit: 2,
            write_budget: usize::MAX,
            rename_error: 0,
            close_error: 0,
            writes: 0,
            reads: 0,
            clock_reads: 0,
            allocations: 0,
            input: fw_api::floating_ball::Input {
                code: 0,
                tag: 0,
                point: floating_ball::model::Point::default(),
            },
            shot_state: 0,
            shot_requests: 0,
            shot_error: 0,
        }
    }
}
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
fn world<T>(f: impl FnOnce(&mut World) -> T) -> T {
    WORLD.with(|w| f(&mut w.borrow_mut()))
}
fn cstr(p: *const u8) -> String {
    unsafe {
        std::ffi::CStr::from_ptr(p.cast())
            .to_str()
            .unwrap()
            .to_owned()
    }
}
fn allocate(parent: u32) -> u32 {
    world(|w| {
        assert!(w.objects.contains_key(&parent));
        let id = w.next_object;
        w.next_object += 1;
        w.allocations += 1;
        w.objects.insert(
            id,
            Object {
                parent,
                flags: 0x12,
                ..Object::default()
            },
        );
        id
    })
}
fn remove_tree(id: u32) {
    let children = world(|w| {
        w.objects
            .iter()
            .filter(|(_, o)| o.parent == id)
            .map(|(&id, _)| id)
            .collect::<Vec<_>>()
    });
    for child in children {
        remove_tree(child);
    }
    world(|w| {
        assert!(w.objects.remove(&id).is_some(), "deleting a dead object");
        w.deleted.push(id);
    });
}
mod screen_rotation {
    pub unsafe fn toggle() -> bool {
        super::world(|w| {
            w.flipped = !w.flipped;
            w.flipped
        })
    }
}
mod capture {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum State {
        Idle,
        Starting,
        Recording,
        Stopping,
    }
    pub fn state() -> State {
        super::world(|w| match w.recording {
            1 => State::Starting,
            2 => State::Recording,
            3 => State::Stopping,
            _ => State::Idle,
        })
    }
    pub fn toggle_recording() {
        super::world(|w| w.recording = if w.recording == 0 { 1 } else { 3 });
    }

    pub fn request_shot_now() -> Result<(), i32> {
        super::world(|w| {
            if matches!(w.shot_state, 1 | 2 | 4) {
                return Err(-16);
            }
            w.shot_requests += 1;
            w.shot_state = 1;
            Ok(())
        })
    }
    pub fn shot_state() -> u8 {
        super::world(|w| w.shot_state)
    }
}
// Text sink only; embedded builds use the production bounded W formatter.
struct W {
    pointer: *mut u8,
    length: usize,
    capacity: usize,
}
impl W {
    fn new(pointer: *mut u8, capacity: usize) -> Self {
        Self {
            pointer,
            capacity,
            length: 0,
        }
    }
    unsafe fn c(&mut self, byte: u8) -> &mut Self {
        if self.length + 1 < self.capacity {
            *self.pointer.add(self.length) = byte;
            self.length += 1;
        }
        self
    }
    unsafe fn s(&mut self, bytes: &[u8]) -> &mut Self {
        for &b in bytes {
            self.c(b);
        }
        self
    }
    unsafe fn n(&mut self, n: u32) -> &mut Self {
        self.s(n.to_string().as_bytes())
    }
    unsafe fn end(&mut self) {
        *self.pointer.add(self.length) = 0;
    }
}
