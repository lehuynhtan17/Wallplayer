//! "Start with Windows" → "High priority".
//!
//! Windows starts the apps in its Run list only after the desktop is up, one by one and with a delay.
//! With High priority on, a Task Scheduler task starts Wallplayer as soon as you sign in instead,
//! at high CPU priority (Wallpaper Engine's high-priority start works the same way). The task belongs
//! to your own account, so no administrator rights are needed.
//!
//! Whichever way Windows starts it, the first moments after a start at sign-in run at high priority
//! (Wallplayer and its web engine), then everything goes back to normal.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows::core::{BSTR, VARIANT};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::TaskScheduler::{ITaskFolder, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, SetPriorityClass, HIGH_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_CREATION_FLAGS, PROCESS_SET_INFORMATION,
};

use crate::settings::{same_path, AUTOSTART_ARG};

// ---------------------------------------------------------------- the sign-in task

/// Runs `f` with the Task Scheduler on a thread of its own (COM wants its own apartment).
fn with_tasks<T: Send + 'static>(f: impl FnOnce(&ITaskFolder, &str) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    std::thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return Err("Task Scheduler unavailable".to_string());
        }
        let r = (|| {
            let svc: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
            let none = VARIANT::default();
            svc.Connect(&none, &none, &none, &none).map_err(|e| e.message())?;
            let user = format!("{}\\{}", svc.ConnectedDomain().map_err(|e| e.message())?, svc.ConnectedUser().map_err(|e| e.message())?);
            let root = svc.GetFolder(&BSTR::from("\\")).map_err(|e| e.message())?;
            f(&root, &user)
        })();
        CoUninitialize();
        r
    })
    .join()
    .unwrap_or_else(|_| Err("Task Scheduler unavailable".to_string()))
}

/// One task per Windows account, so two people on one PC don't share (or block) each other's.
fn task_name(user: &str) -> String {
    format!("Wallplayer - {}", user.rsplit('\\').next().unwrap_or(user))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unesc(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn task_xml(user: &str) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let exe = exe.display().to_string();
    let (exe, dir, user) = (esc(exe.trim_start_matches(r"\\?\")), esc(dir.trim_start_matches(r"\\?\")), esc(user));
    // Priority 1 = high (the default, 7, is below normal and slows the app down for good).
    // ExecutionTimeLimit PT0S: never stopped by the scheduler (the default ends a task after 3 days).
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Wallplayer</Author>
    <Description>Starts Wallplayer when you sign in, before the other startup apps. Turn it off in Wallplayer: Settings, Performance, High priority.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>1</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{exe}</Command><Arguments>{AUTOSTART_ARG}</Arguments><WorkingDirectory>{dir}</WorkingDirectory></Exec>
  </Actions>
</Task>"#
    ))
}

/// The task as Windows has it: (switched on, exe it starts). None = no task.
fn read_task() -> Option<(bool, String)> {
    with_tasks(|root, user| unsafe {
        let Ok(t) = root.GetTask(&BSTR::from(task_name(user))) else { return Ok(None) };
        let on = t.Enabled().map(|b| b.as_bool()).unwrap_or(false);
        let xml = t.Xml().map(|x| x.to_string()).unwrap_or_default();
        let cmd = xml.split("<Command>").nth(1).and_then(|s| s.split("</Command>").next()).unwrap_or("");
        Ok(Some((on, unesc(cmd).trim().trim_matches('"').to_string())))
    })
    .ok()
    .flatten()
}

fn register() -> Result<(), String> {
    with_tasks(|root, user| unsafe {
        let xml = task_xml(user)?;
        let none = VARIANT::default();
        root.RegisterTask(&BSTR::from(task_name(user)), &BSTR::from(xml), TASK_CREATE_OR_UPDATE.0, &none, &none, TASK_LOGON_INTERACTIVE_TOKEN, &none)
            .map(|_| ())
            .map_err(|e| format!("Couldn't set up High priority: {}", e.message()))
    })
}

fn unregister() -> Result<(), String> {
    with_tasks(|root, user| unsafe {
        let name = BSTR::from(task_name(user));
        if root.GetTask(&name).is_err() {
            return Ok(());
        }
        root.DeleteTask(&name, 0).map_err(|e| format!("Couldn't turn High priority off: {}", e.message()))
    })
}

/// A task exists (whichever copy it starts): Wallplayer must not also be in the Run list.
pub fn task_exists() -> bool {
    read_task().is_some()
}

/// On only when the task is switched on and starts THIS copy.
pub fn high_on() -> bool {
    match (read_task(), std::env::current_exe()) {
        (Some((true, cmd)), Ok(me)) => same_path(&cmd, &me),
        _ => false,
    }
}

pub fn set_high(on: bool) -> Result<(), String> {
    if on {
        register()
    } else {
        unregister()
    }
}

/// At start: the task starts another copy or folder (the folder was moved) → point it at this copy.
pub fn heal() -> Option<String> {
    let (_, cmd) = read_task()?;
    let me = std::env::current_exe().ok()?;
    if same_path(&cmd, &me) {
        return None;
    }
    Some(match register() {
        Ok(()) => format!("High priority start pointed to {cmd}; now points to this copy"),
        Err(e) => e,
    })
}

// ---------------------------------------------------------------- high priority while starting

static BOOST: AtomicBool = AtomicBool::new(false);

/// Wallplayer and every process it started (the WebView2 engine: browser, renderer, GPU…).
fn family() -> Vec<u32> {
    let me = unsafe { GetCurrentProcessId() };
    let mut pairs = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return vec![me] };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                pairs.push((e.th32ProcessID, e.th32ParentProcessID));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    let mut out = vec![me];
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        for &(pid, parent) in &pairs {
            if parent == p && pid != 0 && pid != p && !out.contains(&pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
}

fn set_class(class: PROCESS_CREATION_FLAGS) {
    for pid in family() {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
                let _ = SetPriorityClass(h, class);
                let _ = CloseHandle(h);
            }
        }
    }
}

/// Started at sign-in: high priority for Wallplayer and its web engine (which starts a moment later,
/// so it is caught again every second) until `boost_end`.
pub fn boost_start() {
    BOOST.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        let t0 = std::time::Instant::now();
        // never longer than 3 minutes, whatever happens
        while BOOST.load(Ordering::Relaxed) && t0.elapsed() < Duration::from_secs(180) {
            set_class(HIGH_PRIORITY_CLASS);
            std::thread::sleep(Duration::from_secs(1));
        }
        BOOST.store(false, Ordering::Relaxed);
        set_class(NORMAL_PRIORITY_CLASS);
    });
}

/// The skin is up: back to normal priority (a moment later), so games and other apps aren't slowed.
pub fn boost_end_after(delay: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        BOOST.store(false, Ordering::Relaxed);
    });
}
