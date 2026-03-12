fn main() {
    if let Err(err) = timez_service::servers::auth::run(timez_service::runtime::parse_parent_pid()) {
        eprintln!("[timez-auth-service] {err}");
    }
}
