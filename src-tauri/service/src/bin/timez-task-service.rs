fn main() {
    if let Err(err) = timez_service::servers::task::run(timez_service::runtime::parse_parent_pid()) {
        eprintln!("[timez-task-service] {err}");
    }
}
