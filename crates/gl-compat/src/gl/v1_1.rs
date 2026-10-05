//! OpenGL 1.1 entry points.
//!
//! Part of the GL 1.1 module split: each version owns the calls it introduced, so any name can
//! be traced to the version that requires it. `EXPORTS` is asserted by a test, so a module
//! cannot claim a name that does not resolve to a real implementation.

use std::collections::HashMap;
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

// ---- display lists ----------------------------------------------------------------------
// The compatibility renderer records the immediate-mode geometry used by legacy model lists.

const GL_COMPILE: u32 = 0x1300;
const GL_COMPILE_AND_EXECUTE: u32 = 0x1301;
const GL_INVALID_ENUM: u32 = 0x0500;
const GL_INVALID_VALUE: u32 = 0x0501;
const GL_INVALID_OPERATION: u32 = 0x0502;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ListCommand {
    Begin(u32),
    End,
    Vertex([f32; 4]),
    Color([f32; 4]),
    TexCoord([f32; 2]),
    CallList(u32),
}

#[derive(Default)]
struct DisplayLists {
    lists: HashMap<u32, Vec<ListCommand>>,
    compiling: Option<(u32, u32, Vec<ListCommand>)>,
}

static LISTS: OnceLock<Mutex<DisplayLists>> = OnceLock::new();
thread_local! {
    static REPLAY_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct ReplayGuard;

impl ReplayGuard {
    fn enter() -> Self {
        REPLAY_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for ReplayGuard {
    fn drop(&mut self) {
        REPLAY_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

fn lists() -> &'static Mutex<DisplayLists> {
    LISTS.get_or_init(|| Mutex::new(DisplayLists::default()))
}

/// Records a command when compiling a list and returns whether it should execute now.
pub(crate) fn record_command(command: ListCommand) -> bool {
    if REPLAY_DEPTH.with(|depth| depth.get() != 0) {
        return true;
    }
    let mut state = lists().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, mode, commands)) = &mut state.compiling {
        commands.push(command);
        *mode == GL_COMPILE_AND_EXECUTE
    } else {
        true
    }
}

fn replay_list(list: u32, depth: usize) {
    if depth >= 64 {
        crate::errors().set(GL_INVALID_OPERATION);
        return;
    }
    let commands = lists()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .lists
        .get(&list)
        .cloned();
    if let Some(commands) = commands {
        let _replay = ReplayGuard::enter();
        for command in commands {
            if let ListCommand::CallList(nested) = command {
                replay_list(nested, depth + 1);
            } else {
                crate::immediate::replay_list_command(command);
            }
        }
    }
}

static NEXT_LIST: AtomicU32 = AtomicU32::new(1);

#[no_mangle]
pub unsafe extern "C" fn glGenLists(range: i32) -> u32 {
    if range <= 0 {
        return 0;
    }
    NEXT_LIST.fetch_add(range as u32, Ordering::Relaxed)
}

#[no_mangle]
pub unsafe extern "C" fn glNewList(list: u32, mode: u32) {
    if list == 0 {
        crate::errors().set(GL_INVALID_VALUE);
        return;
    }
    if mode != GL_COMPILE && mode != GL_COMPILE_AND_EXECUTE {
        crate::errors().set(GL_INVALID_ENUM);
        return;
    }
    let mut state = lists().lock().unwrap_or_else(|e| e.into_inner());
    if state.compiling.is_some() {
        crate::errors().set(GL_INVALID_OPERATION);
        return;
    }
    state.compiling = Some((list, mode, Vec::new()));
}

#[no_mangle]
pub unsafe extern "C" fn glEndList() {
    let mut state = lists().lock().unwrap_or_else(|e| e.into_inner());
    match state.compiling.take() {
        Some((list, _, commands)) => {
            state.lists.insert(list, commands);
        }
        None => crate::errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCallList(list: u32) {
    if record_command(ListCommand::CallList(list)) {
        replay_list(list, 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteLists(list: u32, range: i32) {
    if range < 0 {
        crate::errors().set(GL_INVALID_VALUE);
        return;
    }
    let mut state = lists().lock().unwrap_or_else(|e| e.into_inner());
    for id in list..list.saturating_add(range as u32) {
        state.lists.remove(&id);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glIsList(list: u32) -> u8 {
    u8::from(
        lists()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .lists
            .contains_key(&list),
    )
}


/// Display-list calls implemented by this compatibility layer.
pub const EXPORTS: &[&str] = &[
    "glGenLists", "glNewList", "glEndList", "glCallList", "glDeleteLists", "glIsList",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_mode_records_geometry_without_executing_it() {
        let _lock = crate::tests::global_test_lock();
        let id = 0x7fff_fff0;
        unsafe { glNewList(id, GL_COMPILE) };
        assert!(!record_command(ListCommand::Begin(0x0004)));
        assert!(!record_command(ListCommand::Vertex([1.0, 2.0, 3.0, 1.0])));
        unsafe { glEndList() };

        let state = lists().lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(state.lists.get(&id).unwrap().len(), 2);
        drop(state);
        unsafe { glDeleteLists(id, 1) };
    }

    #[test]
    fn compile_and_execute_keeps_called_lists_as_calls() {
        let _lock = crate::tests::global_test_lock();
        let source = 0x7fff_fff1;
        let caller = 0x7fff_fff2;
        unsafe { glNewList(source, GL_COMPILE) };
        assert!(!record_command(ListCommand::Color([1.0, 0.5, 0.25, 1.0])));
        unsafe { glEndList() };

        unsafe { glNewList(caller, GL_COMPILE_AND_EXECUTE) };
        unsafe { glCallList(source) };
        unsafe { glEndList() };
        let state = lists().lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(state.lists.get(&caller).unwrap(), &[ListCommand::CallList(source)]);
        drop(state);
        unsafe { glDeleteLists(source, 2) };
    }
}
