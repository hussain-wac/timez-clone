fn main() {
    if let Err(err) = timez_service::servers::quit::run(timez_service::runtime::parse_parent_pid()) {
        eprintln!("[timez-quit-service] {err}");
    }
}
