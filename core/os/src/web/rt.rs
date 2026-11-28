//! The web: engine threads are a Worker's one thread, which can't ask for real time, and there are
//! no audio workgroups. The browser's audio thread is the AudioWorklet's, which only copies the
//! Worker's blocks (the web engine note's decision 1).

use std::time::Duration;

use crate::rt::Joined;

/// The web has none.
pub(crate) struct Workgroup;

impl Workgroup {
    pub(crate) fn none() -> Workgroup {
        Workgroup
    }

    pub(crate) fn recommends(&self) -> Option<usize> {
        None
    }
}

/// None to miss, as on Linux.
pub(crate) fn output_workgroup(_uid: Option<&str>) -> Result<Workgroup, String> {
    Ok(Workgroup)
}

/// A Worker runs at the page's priority: no real time.
pub(crate) fn engine_thread(_period: Option<Duration>, _w: &Workgroup) -> Joined {
    Joined::default()
}

pub(crate) fn end_engine_thread(_w: &Workgroup) {}

pub(crate) fn on_device_thread() -> Joined {
    Joined::default()
}
