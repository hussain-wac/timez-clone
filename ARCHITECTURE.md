# Timez Pro - Architecture Documentation

## Overview

Timez Pro is a desktop time tracking application built with **Tauri** (Rust backend + React/TypeScript frontend). It integrates with a backend API to manage tasks, track time, detect idle periods, and provide activity statistics.

---

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Desktop Application                          │
├─────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────┐    ┌────────────────────────────┐  │
│  │    React Frontend (TSX)    │◄──►│    Tauri Commands (Rust)  │  │
│  │                             │    │                            │  │
│  │  - App.tsx (Main UI)       │    │  - commands.rs            │  │
│  │  - LoginPage.tsx           │    │  - services/tasks.rs      │  │
│  │  - AuthContext.tsx         │    │  - services/auth.rs       │  │
│  │  - App.css                 │    │  - services/activity.rs   │  │
│  └─────────────────────────────┘    └────────────────────────────┘  │
│                   │                              │                    │
│                   ▼                              ▼                    │
│  ┌───────────────────────────────────────────────────────────────┐   │
│  │                    Rust Backend Core                          │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐   │   │
│  │  │ timer_state │  │    idle     │  │         api         │   │   │
│  │  │   .rs       │  │    .rs      │  │         .rs         │   │   │
│  │  └─────────────┘  └─────────────┘  └─────────────────────┘   │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐   │   │
│  │  │  models.rs  │  │ services/   │  │     lib.rs          │   │   │
│  │  │             │  │ quit.rs     │  │   (Tauri Setup)     │   │   │
│  │  └─────────────┘  └─────────────┘  └─────────────────────┘   │   │
│  └───────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │   Backend API   │
                    │  (FastAPI)      │
                    │ 192.168.3.163   │
                    │    :8000        │
                    └─────────────────┘
```

---

## Technology Stack

### Frontend
| Technology | Purpose |
|------------|---------|
| React 19 | UI framework |
| TypeScript | Type safety |
| Tailwind CSS 4 | Styling |
| Vite 7 | Build tool |
| @tauri-apps/api | Tauri IPC communication |

### Backend (Desktop)
| Technology | Purpose |
|------------|---------|
| Tauri 2 | Desktop runtime |
| Rust | Backend logic |
| serde | Serialization |
| chrono | Date/time handling |
| dbus | System idle detection (Linux) |
| ureq | HTTP client |

---

## Module Analysis

### Frontend Modules

#### `src/main.tsx`
Entry point that sets up React with AuthProvider and conditional rendering between LoginPage and App.

#### `src/AuthContext.tsx`
Manages authentication state:
- Stores user info, access token
- Handles Google OAuth login flow
- Persists token in localStorage
- Provides `loginViaBrowser()` and `logout()` methods

#### `src/LoginPage.tsx`
Simple login UI with Google OAuth button. Initiates OAuth flow via AuthContext.

#### `src/App.tsx`
Main application UI with:
- **Left sidebar**: User profile, total runtime, task breakdown
- **Right panel**: Task list with search, timer controls
- **Modals**: Idle time alert, quit confirmation
- **Event listeners**: Listens to Tauri events (`idle-detected`, `timer-stopped`, `activity-update`)

---

### Rust Backend Modules

#### `src-tauri/src/main.rs`
- Single-instance enforcement (prevents multiple app instances)
- Calls `timez_pro_lib::run()`

#### `src-tauri/src/lib.rs`
Main Tauri application setup:
- **State management**: `AuthToken`, `ActivityState`, `TimerState`
- **System tray**: Menu with Show/Pause-Resume/Quit options
- **Background threads**: 
  - Sync thread (10-minute intervals)
  - Idle monitor thread (60-second threshold)
- **Window events**: Hide instead of close (stays in tray)
- **Command handlers**: 13 Tauri commands exposed to frontend

#### `src-tauri/src/commands.rs`
Thin wrapper around service functions. Each command delegates to corresponding service module.

#### `src-tauri/src/models.rs`
Data structures for frontend-backend communication:
```rust
Task, TimerStatus, IdleEvent, ActivityStats, AuthUser, AuthResponse
```

#### `src-tauri/src/api.rs`
HTTP client for backend API (`http://192.168.3.163:8000`):
- `google_login()` - Exchange Google ID token for access token
- `get_me()` - Validate token, get user info
- `list_tasks()` - Fetch tasks with elapsed times
- `start_timer()`, `stop_timer()` - Timer control
- `get_status()` - Get current running task
- `get_summary()` - Get time summary report
- `google_oauth_via_browser()` - OAuth2 flow with local callback server

#### `src-tauri/src/timer_state.rs`
Local timer state management:
- **Cached tasks**: Local copy of tasks from API
- **Running task tracking**: Local tracking of active timer
- **Base elapsed**: Time accumulated before current run
- **Sync logic**: Background thread syncs every 10 minutes
- **Idle resume**: Adds idle duration to task time

#### `src-tauri/src/idle.rs`
Idle detection via D-Bus (Linux):
- Polls GNOME Mutter IdleMonitor every 2 seconds
- 60-second idle threshold
- Emits `idle-detected` event when user returns
- Automatically stops timer when idle detected
- Tracks activity stats (active vs idle time)

#### `src-tauri/src/services/`
| Module | Responsibility |
|--------|----------------|
| `tasks.rs` | Task CRUD, timer control |
| `auth.rs` | OAuth flow, token validation, logout |
| `activity.rs` | Activity statistics retrieval |
| `quit.rs` | Application quit handling |

---

## Data Flow

### Authentication Flow
```
1. User clicks "Sign in with Google" (LoginPage.tsx)
2. AuthContext.loginViaBrowser() → Tauri command "start_google_auth"
3. Rust opens system browser → Google OAuth consent page
4. Google redirects to local callback server (api.rs)
5. Backend exchanges code for ID token → API for access token
6. Token stored in AuthToken state + localStorage
7. TimerState syncs tasks from API
8. App renders main UI with tasks
```

### Timer Flow
```
1. User clicks play button (App.tsx)
2. Tauri command "start_timer" → services/tasks.rs
3. api::start_timer() calls backend API
4. timer_state updates local state (running_task_id, timer_started_at)
5. Frontend receives updated task list
6. Local interval ticks UI every second for smooth display
7. Background sync updates from API every 10 minutes
```

### Idle Detection Flow
```
1. Idle monitor polls D-Bus every 2 seconds
2. If idle > 60 seconds:
   - Stop current timer via API
   - Emit "idle-detected" event
3. Frontend shows modal to keep/discard idle time
4. On user action, call "add_idle_time" or "discard_idle_time"
5. Timer resumes for selected task
```

---

## State Management

### Frontend State
- **React Context**: `AuthContext` for authentication
- **useState**: Local UI state (tasks, selectedTaskId, modals)
- **useEffect**: Event listeners, polling intervals

### Backend State (Tauri State)
| State | Type | Purpose |
|-------|------|---------|
| `AuthToken` | `Mutex<AuthTokenState>` | Access token storage |
| `ActivityState` | `Mutex<ActivityTracker>` | Active/idle time tracking |
| `TimerState` | `Mutex<TimerStateInner>` | Task cache, running timer |

---

## Key Design Decisions

1. **Local Timer State**: Timer runs locally for instant UI updates; API sync happens periodically
2. **Idle Detection**: Uses D-Bus (Linux-specific) for system-wide idle detection
3. **Single Instance**: Prevents multiple app instances via `single-instance` crate
4. **System Tray**: App hides to tray on close instead of exiting
5. **OAuth via Browser**: Full OAuth2 flow with local callback server
6. **Background Sync**: 10-minute sync interval reduces API load

---

## API Endpoints (Backend)

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/auth/google` | POST | Exchange Google token |
| `/api/auth/me` | GET | Validate token, get user |
| `/api/tasks` | GET | List tasks |
| `/api/tasks/{id}/start` | POST | Start timer |
| `/api/tasks/{id}/stop` | POST | Stop timer |
| `/api/status` | GET | Current running status |
| `/api/report/summary` | GET | Time summary |

---

## Build & Distribution

- **Frontend**: Vite builds to `dist/`
- **Backend**: Cargo builds Rust to binary
- **Bundle**: Tauri bundles as `.deb` (Linux)
- **Services**: Additional systemd-style services bundled as resources

---

## Security Considerations

- Tokens stored in localStorage (frontend) and Tauri state (backend)
- API requests include Bearer token
- CSP disabled in tauri.conf.json (for development)
- Single-instance prevents token conflicts

---

## Future Improvements

- Add encryption for stored tokens
- Implement Windows/macOS idle detection
- Add offline mode with local storage
- Implement task creation/deletion from app
- Add reporting and analytics UI
