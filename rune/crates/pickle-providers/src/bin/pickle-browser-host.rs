#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    pickle_providers::browser::run_native_host();
}
