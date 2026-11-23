//! "Start with Windows" → "High priority" is a Windows Task Scheduler task (see ../boot.rs).
//! Linux and macOS have no such thing: the login start (unix/autostart.rs) is the only start, and
//! the first moments after it run at the normal priority (raising it needs administrator rights).

use std::time::Duration;

#[allow(dead_code)]
pub fn task_exists() -> bool {
    false
}

pub fn high_on() -> bool {
    false
}

#[allow(dead_code)]
pub fn set_high(on: bool) -> Result<(), String> {
    if on {
        Err("High priority is only available on Windows".into())
    } else {
        Ok(())
    }
}

#[allow(dead_code)]
pub fn heal() -> Option<String> {
    None
}

pub fn boost_start() {}

pub fn boost_end_after(_delay: Duration) {}
