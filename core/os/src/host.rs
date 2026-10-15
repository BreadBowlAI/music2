//! Who this machine is, for journals and messages (FMT-20, WRITE-16, WRITE-18).

use crate::os;

/// This machine's lasting id: the hardware UUID on macOS, `/etc/machine-id` (else
/// `/var/lib/dbus/machine-id`) on Linux. None when the OS has none; the lock then tells machines
/// apart by their host name.
pub fn machine_id() -> Option<String> {
    os::machine_id()
}

/// The host name, for messages: it can change with the network.
pub fn name() -> String {
    os::host_name().unwrap_or_else(|| "this computer".into())
}

/// `(hour, minute)` of a time in seconds since the epoch, in local time; None when the OS can't
/// tell.
pub fn local_time(unix_secs: u64) -> Option<(u8, u8)> {
    os::local_time(unix_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_has_a_name_an_id_and_a_clock() {
        let id = machine_id();
        assert!(id.as_deref().is_none_or(|i| !i.is_empty() && i.trim() == i), "{id:?}");
        assert_eq!(machine_id(), id, "lasting");
        assert!(!name().is_empty());
        let (h, m) = local_time(1_760_000_000).unwrap();
        assert!(h < 24 && m < 60, "{h}:{m}");
        // 1,760,000,000 s is 08:53:20 UTC, and time zones are whole quarter hours from UTC.
        assert!([8, 23, 38, 53].contains(&m), "{h}:{m}");
    }
}
