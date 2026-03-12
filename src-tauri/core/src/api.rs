use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use crate::models::{AuthResponse, Task};

const BASE_URL: &str = "http://192.168.3.163:8000";

/// Shared auth token state
pub struct AuthTokenState {
    pub access_token: Option<String>,
}

pub type AuthToken = Mutex<AuthTokenState>;

impl AuthTokenState {
    pub fn new() -> Self {
        Self { access_token: None }
    }
}

fn auth_header(token: &Option<String>) -> Option<String> {
    token.as_ref().map(|t| format!("Bearer {}", t))
}

#[derive(Debug, Deserialize)]
pub struct ApiTask {
    pub id: i64,
    pub name: String,
    pub max_hours: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ApiStatusTask {
    pub id: i64,
    pub name: String,
    pub max_hours: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ApiStatus {
    pub running: bool,
    pub task: Option<ApiStatusTask>,
    pub time_entry_id: Option<i64>,
    pub elapsed_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SummaryTask {
    pub task_id: i64,
    pub task_name: String,
    pub total_seconds: i64,
}

#[derive(Debug, Deserialize)]
pub struct SummaryReport {
    pub tasks: Vec<SummaryTask>,
    pub total_seconds: i64,
}

/// Send Google ID token to backend, receive access token + user info
pub fn google_login(google_id_token: &str) -> Result<AuthResponse, String> {
    let resp = ureq::post(&format!("{}/api/auth/google", BASE_URL))
        .send_json(ureq::json!({ "token": google_id_token }))
        .map_err(|e| format!("Auth error: {}", e))?;
    resp.into_json().map_err(|e| format!("Parse error: {}", e))
}

/// Validate stored token by calling /api/auth/me
pub fn get_me(token: &str) -> Result<crate::models::AuthUser, String> {
    let resp = ureq::get(&format!("{}/api/auth/me", BASE_URL))
        .set("Authorization", &format!("Bearer {}", token))
        .call()
        .map_err(|e| format!("Auth error: {}", e))?;
    resp.into_json().map_err(|e| format!("Parse error: {}", e))
}

/// Fetches tasks, merges with summary (elapsed) and status (running).
pub fn list_tasks(token: &Option<String>) -> Result<Vec<Task>, String> {
    let mut req = ureq::get(&format!("{}/api/tasks", BASE_URL));
    if let Some(header) = auth_header(token) {
        req = req.set("Authorization", &header);
    }
    let resp = req.call().map_err(|e| format!("API error: {}", e))?;
    let api_tasks: Vec<ApiTask> = resp.into_json().map_err(|e| format!("Parse error: {}", e))?;

    // Get elapsed times from summary report
    let summary = get_summary(token).unwrap_or(SummaryReport {
        tasks: vec![],
        total_seconds: 0,
    });
    let mut summary_map = std::collections::HashMap::with_capacity(summary.tasks.len());
    for st in &summary.tasks {
        summary_map.insert(st.task_id, st.total_seconds);
    }

    // Get running status
    let status = get_status(token).ok();
    let (running_task_id, running_elapsed) = match status.as_ref().filter(|s| s.running) {
        Some(s) => (
            s.task.as_ref().map(|t| t.id),
            s.elapsed_seconds.unwrap_or(0),
        ),
        None => (None, 0),
    };

    let mut tasks = Vec::with_capacity(api_tasks.len());
    for t in api_tasks {
        let is_running = running_task_id == Some(t.id);
        let summary_elapsed = summary_map.get(&t.id).copied().unwrap_or(0);
        let elapsed = if is_running {
            summary_elapsed + running_elapsed
        } else {
            summary_elapsed
        };
        tasks.push(Task {
            id: t.id,
            name: t.name,
            budget_secs: ((t.max_hours.unwrap_or(8.0)) * 3600.0) as i64,
            elapsed_secs: elapsed,
            running: is_running,
        });
    }

    Ok(tasks)
}

pub fn start_timer(task_id: i64, token: &Option<String>) -> Result<(), String> {
    let mut req = ureq::post(&format!("{}/api/tasks/{}/start", BASE_URL, task_id));
    if let Some(header) = auth_header(token) {
        req = req.set("Authorization", &header);
    }
    req.call().map_err(|e| format!("API error: {}", e))?;
    Ok(())
}

pub fn stop_timer(task_id: i64, token: &Option<String>) -> Result<(), String> {
    let mut req = ureq::post(&format!("{}/api/tasks/{}/stop", BASE_URL, task_id));
    if let Some(header) = auth_header(token) {
        req = req.set("Authorization", &header);
    }
    req.call().map_err(|e| format!("API error: {}", e))?;
    Ok(())
}

pub fn get_status(token: &Option<String>) -> Result<ApiStatus, String> {
    let mut req = ureq::get(&format!("{}/api/status", BASE_URL));
    if let Some(header) = auth_header(token) {
        req = req.set("Authorization", &header);
    }
    let resp = req.call().map_err(|e| format!("API error: {}", e))?;
    resp.into_json().map_err(|e| format!("Parse error: {}", e))
}

fn get_summary(token: &Option<String>) -> Result<SummaryReport, String> {
    let mut req = ureq::get(&format!("{}/api/report/summary", BASE_URL));
    if let Some(header) = auth_header(token) {
        req = req.set("Authorization", &header);
    }
    let resp = req.call().map_err(|e| format!("API error: {}", e))?;
    resp.into_json().map_err(|e| format!("Parse error: {}", e))
}

// ---- OAuth2 Authorization Code Flow via System Browser ----

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    id_token: Option<String>,
}

/// Runs the full OAuth2 flow:
/// 1. Start local HTTP server on a random port
/// 2. Open the system browser to Google's consent screen
/// 3. Wait for the redirect with the auth code
/// 4. Exchange the code for an ID token
/// 5. Send the ID token to our FastAPI backend
pub fn google_oauth_via_browser(client_id: &str, client_secret: &str) -> Result<AuthResponse, String> {
    // 1. Bind to a random port on localhost
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to start local server: {}", e))?;
    let port = listener.local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{}", port);

    // Set a timeout so we don't block forever
    listener.set_nonblocking(false).ok();

    // 2. Build Google OAuth URL
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={}&\
         redirect_uri={}&\
         response_type=code&\
         scope=email%20profile%20openid&\
         access_type=offline&\
         prompt=consent",
        urlencod(client_id),
        urlencod(&redirect_uri),
    );

    // 3. Open system browser
    open::that(&auth_url).map_err(|e| format!("Failed to open browser: {}", e))?;

    eprintln!("[auth] Waiting for Google OAuth callback on port {}...", port);

    // 4. Wait for the callback (with 2-minute timeout)
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("Failed to set blocking: {}", e))?;

    let code = wait_for_auth_code(&listener)?;

    eprintln!("[auth] Received auth code, exchanging for tokens...");

    // 5. Exchange the auth code for tokens
    let token_resp = ureq::post("https://oauth2.googleapis.com/token")
        .send_form(&[
            ("code", &code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", &redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .map_err(|e| format!("Token exchange error: {}", e))?;

    let google_tokens: GoogleTokenResponse = token_resp
        .into_json()
        .map_err(|e| format!("Failed to parse token response: {}", e))?;

    let id_token = google_tokens
        .id_token
        .ok_or_else(|| "No id_token in Google response".to_string())?;

    eprintln!("[auth] Got ID token, sending to backend...");

    // 6. Send the ID token to our backend
    google_login(&id_token)
}

/// Waits for the OAuth redirect on the local TCP listener, parses the auth code,
/// and sends a nice HTML response to the browser.
fn wait_for_auth_code(listener: &TcpListener) -> Result<String, String> {
    // Accept one connection (blocking, with timeout via socket option)
    let (mut stream, _) = listener.accept()
        .map_err(|e| format!("Failed to accept connection: {}", e))?;

    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)
        .map_err(|e| format!("Failed to read request: {}", e))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the GET request line to extract the code parameter
    // Format: GET /?code=...&scope=... HTTP/1.1
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");

    // Check for error
    if let Some(error) = extract_query_param(path, "error") {
        let html = format!(
            "<html><body style='font-family:sans-serif;text-align:center;padding:60px'>\
             <h2 style='color:#dc2626'>Authentication Failed</h2>\
             <p>Error: {}</p>\
             <p style='color:#666'>You can close this tab.</p></body></html>",
            error
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        stream.write_all(response.as_bytes()).ok();
        stream.flush().ok();
        return Err(format!("OAuth error: {}", error));
    }

    let code = extract_query_param(path, "code")
        .ok_or_else(|| "No authorization code in callback".to_string())?;

    // Send a success page to the browser
    let html = "<html><body style='font-family:sans-serif;text-align:center;padding:60px'>\
        <h2 style='color:#7c3aed'>Authentication Successful</h2>\
        <p style='color:#666'>You can close this tab and return to the app.</p>\
        <script>setTimeout(()=>window.close(),2000)</script></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    stream.write_all(response.as_bytes()).ok();
    stream.flush().ok();

    Ok(code)
}

/// Extract a query parameter value from a URL path like /?code=abc&scope=xyz
fn extract_query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if kv.next() == Some(key) {
            return kv.next().map(|v| urldecd(v));
        }
    }
    None
}

/// Minimal URL-encode (just the chars that matter for OAuth params)
fn urlencod(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('/', "%2F")
        .replace(':', "%3A")
}

/// Minimal URL-decode
fn urldecd(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}
