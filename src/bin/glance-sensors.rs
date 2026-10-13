//! Glance's sensors (see `sensing`): a program of their own, beside `glance.exe`, started elevated through Glance's
//! task. It shows no window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    glance_lib::run_sensors()
}
