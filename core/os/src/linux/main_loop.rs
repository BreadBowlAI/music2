//! Linux: a job queue. There is no process-wide run loop for plugins to post to: they get main
//! thread time only from the host (CLAP's fd and timer extensions, step 9). So the app's loop is a
//! thread of its own, and GTK keeps the main thread.

use std::io;
use std::time::Duration;

use crate::main_loop::{Job, Queue, Where};

pub(crate) const APP_ON_PROCESS_MAIN: bool = false;

pub(crate) struct Loop(Queue);

impl Loop {
    pub(crate) fn start(at: Where) -> io::Result<Loop> {
        match at {
            Where::ProcessMain if super::is_process_main() => Queue::start(None).map(Loop),
            Where::ProcessMain => Err(io::Error::other("the process's main loop starts on the process's main thread")),
            Where::AppMain => Queue::start(Some("m2-app-main")).map(Loop),
            Where::Thread(name) => Queue::start(Some(name)).map(Loop),
        }
    }

    pub(crate) fn post(&self, job: Job) {
        self.0.post(job)
    }

    pub(crate) fn is_current(&self) -> bool {
        self.0.is_current()
    }

    pub(crate) fn run_for(&self, most: Duration) {
        self.0.run_for(most)
    }
}
