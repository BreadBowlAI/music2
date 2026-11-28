//! No kernel host: a build without the `wasmtime` feature, and without another host yet. It runs
//! no kernel: every module is refused when it is loaded, in words. So the runtime builds without
//! wasmtime, as the web's build will, before the web has its own host.

use std::time::Duration;

use anyhow::{Result, bail};

use crate::abi::Call;
use crate::host::KernelHost;
use crate::sandbox::Assets;

pub(crate) struct NoHost;

/// No module: none is ever compiled.
#[derive(Clone)]
pub(crate) enum NoModule {}

/// No sandbox: none is ever made.
pub(crate) enum NoSandbox {}

/// No export: none is ever found.
#[derive(Clone)]
pub(crate) enum NoExport {}

impl KernelHost for NoHost {
    type Module = NoModule;
    type Sandbox = NoSandbox;

    fn compile(_bytes: &[u8], _checked: bool) -> Result<NoModule> {
        bail!("this build of music2 has no kernel host, so it plays no kernels: build m2-runtime with its `wasmtime` feature")
    }

    fn checked(module: &NoModule) -> bool {
        match *module {}
    }

    fn instantiate(module: &NoModule, _limit: Duration) -> Result<NoSandbox> {
        match *module {}
    }
}

impl crate::host::Sandbox for NoSandbox {
    type Export = NoExport;

    fn export(&mut self, _name: &str, _call: Call) -> Result<NoExport> {
        match *self {}
    }

    fn call(&mut self, _f: &NoExport, _args: [u32; 4]) -> Result<u32> {
        match *self {}
    }

    fn limit(&mut self, _limit: Duration) {
        match *self {}
    }

    fn assets(&mut self) -> &mut Assets {
        match *self {}
    }

    fn seal(&mut self) -> Result<()> {
        match *self {}
    }

    fn size(&self) -> usize {
        match *self {}
    }

    fn write(&mut self, _at: usize, _bytes: &[u8]) {
        match *self {}
    }

    fn read(&self, _at: usize, _out: &mut [u8]) {
        match *self {}
    }
}
