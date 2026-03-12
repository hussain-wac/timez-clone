use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

const AUTH_STORE_PATH: &str = "/tmp/timez-auth-store.json";

#[derive(Debug, Serialize, Deserialize, Default)]
struct AuthStore {
    access_token: Option<String>,
}

pub fn read_token() -> Option<String> {
    let path = Path::new(AUTH_STORE_PATH);
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str::<AuthStore>(&contents)
        .ok()
        .and_then(|store| store.access_token)
}

pub fn write_token(token: Option<String>) -> Result<(), String> {
    let store = AuthStore { access_token: token };
    let payload = serde_json::to_string(&store).map_err(|err| err.to_string())?;
    fs::write(AUTH_STORE_PATH, payload).map_err(|err| err.to_string())
}
