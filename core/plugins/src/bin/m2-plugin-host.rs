//! The plugin host process, for tests (HOST-3, D2). Programs run themselves again with
//! `--m2-plugin-host` instead; a test can't, as libtest owns its `main`. `m2_plugins::Remote::for_tests`
//! finds this one beside the test.

fn main() {
    m2_plugins::serve_host()
}
