use timez_core::protocol::{Request, ResponseData};

use crate::runtime;
use crate::ServiceKind;

pub fn run(parent_pid: Option<u32>) -> Result<(), String> {
    runtime::run_server(ServiceKind::Quit.socket_path(), parent_pid, move |request| match request {
        Request::Shutdown => Ok(ResponseData::Unit),
        _ => Err("Unsupported request for quit service".to_string()),
    })
}
