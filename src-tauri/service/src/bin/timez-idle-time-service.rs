fn main() {
    if let Err(err) = timez_service::servers::idle_time::run(timez_service::runtime::parse_parent_pid()) {
        eprintln!("[timez-idle-time-service] {err}");
    }
}
