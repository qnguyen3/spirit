cfg_if::cfg_if! {
    if #[cfg(target_os = "windows")] {
        pub use super::windows::status_item::{StatusItemHandle, install};
    } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
        pub use super::linux::status_item::{StatusItemHandle, install};
    }
}
