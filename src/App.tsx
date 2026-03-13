import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAuth } from "./AuthContext";

interface IdleEvent {
  idle_duration_secs: number;
  task_id: number;
  task_name: string;
  tracking_active: boolean;
}

interface Task {
  id: number;
  name: string;
  budget_secs: number;
  elapsed_secs: number;
  running: boolean;
}

const EIGHT_HOURS = 8 * 60 * 60;

function formatHms(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return `${h}:${m.toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}`;
}

function formatIdleDuration(secs: number): string {
  const mins = Math.floor(secs / 60);
  if (mins < 1) return `${secs} sec`;
  if (mins === 1) return "1 minute";
  const h = Math.floor(mins / 60);
  const remainMins = mins % 60;
  if (h > 0) return `${h}h ${remainMins}m`;
  return `${mins} minutes`;
}

function App() {
  const { user, logout } = useAuth();
  const [tasks, setTasks] = useState<Task[]>([]);
  const [selectedTaskId, setSelectedTaskId] = useState<number | null>(null);
  const [taskSearch, setTaskSearch] = useState("");
  const [idleEvent, setIdleEvent] = useState<IdleEvent | null>(null);
  const [idleTaskId, setIdleTaskId] = useState<number | null>(null);
  const [quitConfirmOpen, setQuitConfirmOpen] = useState(false);
  const [quitHasRunning, setQuitHasRunning] = useState(false);
  const [quitError, setQuitError] = useState<string | null>(null);
  const [crashRecoveryOpen, setCrashRecoveryOpen] = useState(false);
  const [crashRecoveredTaskId, setCrashRecoveredTaskId] = useState<number | null>(null);
  const [syncNotification, setSyncNotification] = useState<string | null>(null);

  const activity = {
    active_secs: 0,
    idle_secs: 0,
    total_secs: 0,
    activity_percent: 100,
  };

  const refreshTasks = useCallback(async () => {
    try {
      const result = await invoke<Task[]>("list_tasks");
      setTasks(result);
    } catch {
      // API may be unreachable
    }
  }, []);

  useEffect(() => {
    refreshTasks();
  }, [refreshTasks]);

  useEffect(() => {
    const id = setInterval(async () => {
      try {
        const event = await invoke<IdleEvent | null>("get_idle_event");
        setIdleEvent((prev) => {
          if (!event) {
            return null;
          }
          if (
            prev &&
            prev.task_id === event.task_id &&
            prev.idle_duration_secs === event.idle_duration_secs &&
            prev.tracking_active === event.tracking_active
          ) {
            return prev;
          }
          return event;
        });
        if (event) {
          setIdleTaskId((prev) => prev ?? event.task_id);
        }
        if (event?.tracking_active) {
          refreshTasks();
        }
      } catch {
        // backend may be restarting
      }
    }, 1000);

    return () => clearInterval(id);
  }, [refreshTasks]);

  // Sync with backend every 5 seconds
  useEffect(() => {
    const id = setInterval(() => {
      refreshTasks();
    }, 5000);
    return () => clearInterval(id);
  }, [refreshTasks]);

  // Locally tick the running task every second for smooth display
  useEffect(() => {
    const id = setInterval(() => {
      setTasks((prev) =>
        prev.map((t) =>
          t.running ? { ...t, elapsed_secs: t.elapsed_secs + 1 } : t,
        ),
      );
    }, 1000);
    return () => clearInterval(id);
  }, []);

  // Listen for events from Rust backend
  useEffect(() => {
    const unlisten1 = listen<IdleEvent>("idle-detected", (event) => {
      setIdleEvent(event.payload);
      setIdleTaskId((prev) => prev ?? event.payload.task_id);
      refreshTasks();
    });

    const unlisten2 = listen("timer-stopped", () => {
      refreshTasks();
    });

    const unlisten4 = listen<boolean>("request-quit-confirm", (event) => {
      setQuitError(null);
      setQuitHasRunning(!!event.payload);
      setQuitConfirmOpen(true);
    });

    const unlisten5 = listen<{ task_id: number; action?: string }>("crash-recovery-complete", (event) => {
      setCrashRecoveredTaskId(event.payload.task_id);
      setCrashRecoveryOpen(true);
    });

    const unlisten6 = listen<{ task_id: number; reason: string }>("time-discarded", (event) => {
      setCrashRecoveredTaskId(event.payload.task_id);
      setCrashRecoveryOpen(true);
    });

    const unlistenSync = listen("sync-in-progress", () => {
      setSyncNotification("Time sync in progress...");
      setTimeout(() => setSyncNotification(null), 3000);
    });

    const unlistenMidnight = listen("midnight-reset", () => {
      refreshTasks();
      setSyncNotification("Timer reset at midnight");
      setTimeout(() => setSyncNotification(null), 5000);
    });

    return () => {
      unlisten1.then((fn) => fn());
      unlisten2.then((fn) => fn());
      unlisten4.then((fn) => fn());
      unlisten5.then((fn) => fn());
      unlisten6.then((fn) => fn());
      unlistenSync.then((fn) => fn());
      unlistenMidnight.then((fn) => fn());
    };
  }, [refreshTasks]);

  const toggleTimer = async (taskId: number) => {
    const task = tasks.find((t) => t.id === taskId);
    if (task?.running) {
      const result = await invoke<Task[]>("stop_timer");
      setTasks(result);
    } else {
      const result = await invoke<Task[]>("start_timer", { taskId });
      setTasks(result);
    }
    setSelectedTaskId(taskId);
  };

  const handleConfirmQuit = async () => {
    setQuitError(null);
    if (quitHasRunning) {
      try {
        await invoke<Task[]>("stop_timer");
      } catch {
        setQuitError("Failed to stop the running timer. Please try again.");
        return;
      }
    }
    await invoke("quit_app");
  };

  const handleKeepIdleTime = async () => {
    if (!idleEvent || idleEvent.tracking_active || !idleTaskId) return;
    const result = await invoke<Task[]>("add_idle_time", {
      taskId: idleTaskId,
      durationSecs: idleEvent.idle_duration_secs,
    });
    setTasks(result);
    await invoke("resolve_idle_event");
    setIdleEvent(null);
    setIdleTaskId(null);
  };

  const handleDiscardIdleTime = async () => {
    if (!idleEvent || idleEvent.tracking_active) return;
    const result = await invoke<Task[]>("discard_idle_time", {
      taskId: idleEvent.task_id,
    });
    setTasks(result);
    await invoke("resolve_idle_event");
    setIdleEvent(null);
    setIdleTaskId(null);
  };

  const filteredTasks = tasks.filter((t) =>
    t.name.toLowerCase().includes(taskSearch.toLowerCase()),
  );

  const selectedTask = tasks.find((t) => t.id === selectedTaskId);
  const totalDaySeconds = tasks.reduce((sum, t) => sum + t.elapsed_secs, 0);

  // Activity color based on percentage
  const activityColor =
    activity.activity_percent >= 80
      ? "text-green-600"
      : activity.activity_percent >= 50
        ? "text-yellow-600"
        : "text-red-600";

  return (
    <div className="h-screen flex bg-gray-100 text-gray-800 select-none overflow-hidden">
      {/* Sync notification */}
      {syncNotification && (
        <div className="fixed top-4 right-4 bg-blue-600 text-white px-4 py-2 rounded-lg shadow-lg z-50 animate-fade-in">
          {syncNotification}
        </div>
      )}
      {/* Left sidebar */}
      <div className="w-72 bg-white border-r border-gray-200 flex flex-col">
        {/* User profile */}
        {user && (
          <div className="flex items-center gap-3 px-6 pt-4 pb-3 border-b border-gray-200">
            {user.picture ? (
              <img
                src={user.picture}
                alt={user.name || "User"}
                className="w-8 h-8 rounded-full"
                referrerPolicy="no-referrer"
              />
            ) : (
              <div className="w-8 h-8 rounded-full bg-purple-200 flex items-center justify-center text-purple-700 text-sm font-medium">
                {(user.name || user.email)[0].toUpperCase()}
              </div>
            )}
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-gray-800 truncate">
                {user.name || user.email}
              </div>
              <div className="text-xs text-gray-400 truncate">{user.email}</div>
            </div>
            <button
              onClick={logout}
              className="text-gray-400 hover:text-red-500 transition-colors shrink-0"
              title="Logout"
            >
              <svg
                className="w-4 h-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"
                />
              </svg>
            </button>
          </div>
        )}

        {/* Today's total */}
        <div className="px-6 pt-6 pb-4">
          <div className="text-xs uppercase tracking-wider text-gray-400 mb-1">
            Total Run Time
          </div>
          <div className="text-4xl font-mono font-semibold text-black">
            {formatHms(totalDaySeconds)}
          </div>
          <div className="text-xs text-gray-400 mt-1">
            / {formatHms(EIGHT_HOURS)}
          </div>
          <div className="mt-2 h-2 bg-gray-200 rounded-full overflow-hidden">
            <div
              className={`h-full rounded-full transition-all duration-500 ${
                totalDaySeconds >= EIGHT_HOURS ? "bg-red-500" : "bg-purple-600"
              }`}
              style={{
                width: `${Math.min((totalDaySeconds / EIGHT_HOURS) * 100, 100)}%`,
              }}
            />
          </div>
        </div>

        {/* Per-task breakdown */}
        <div className="px-6 pb-4 flex-1 overflow-y-auto">
          <div className="text-xs uppercase tracking-wider text-gray-400 mb-2">
            Task Breakdown
          </div>
          <div className="space-y-2">
            {tasks
              .filter((t) => t.elapsed_secs > 0)
              .map((t) => (
                <div
                  key={t.id}
                  className="flex items-center justify-between text-xs"
                >
                  <span
                    className={`truncate mr-2 max-w-[140px] ${t.running ? "text-purple-700 font-semibold" : "text-gray-600"}`}
                  >
                    {t.name}
                  </span>
                  <span
                    className={`font-mono shrink-0 ${t.running ? "text-purple-700 font-semibold" : "text-gray-800"}`}
                  >
                    {formatHms(t.elapsed_secs)}
                  </span>
                </div>
              ))}
            {tasks.every((t) => t.elapsed_secs === 0) && (
              <div className="text-xs text-gray-400 italic">
                No time tracked yet
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Right panel - Tasks */}
      <div className="flex-1 flex flex-col bg-gray-50">
        {/* Task search */}
        <div className="px-4 py-3 bg-white border-b border-gray-200">
          <div className="relative">
            <svg
              className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
              />
            </svg>
            <input
              type="text"
              placeholder="Search task"
              value={taskSearch}
              onChange={(e) => setTaskSearch(e.target.value)}
              className="w-full pl-9 pr-3 py-2 text-sm border border-gray-300 rounded-md focus:outline-none focus:ring-1 focus:ring-purple-500 focus:border-purple-500"
            />
          </div>
        </div>

        {/* Task list */}
        <div className="flex-1 overflow-y-auto">
          {filteredTasks.map((task) => {
            const isActive = task.running;
            const isExceeded = task.elapsed_secs > task.budget_secs;

            return (
              <div
                key={task.id}
                onClick={() => setSelectedTaskId(task.id)}
                className={`flex items-center justify-between px-4 py-3 border-b border-gray-200 cursor-pointer transition-colors ${
                  isActive
                    ? "bg-purple-900 text-white"
                    : selectedTaskId === task.id
                      ? "bg-purple-50"
                      : "bg-white hover:bg-gray-50"
                }`}
              >
                <div className="flex-1 min-w-0 mr-3">
                  <span
                    className={`text-sm truncate block ${isActive ? "text-white" : "text-gray-800"}`}
                  >
                    {task.name}
                  </span>
                </div>

                <div className="flex items-center gap-3 shrink-0">
                  {isExceeded && (
                    <span
                      className={`text-[10px] px-2 py-0.5 rounded-full font-semibold tracking-wide ${
                        isActive
                          ? "bg-red-500/20 text-red-200"
                          : "bg-red-100 text-red-700"
                      }`}
                    >
                      Time Exceeded
                    </span>
                  )}
                  <span
                    className={`text-sm font-mono ${isActive ? "text-purple-200" : "text-gray-500"}`}
                  >
                    {formatHms(task.elapsed_secs)} /{" "}
                    {formatHms(task.budget_secs)}
                  </span>

                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleTimer(task.id);
                    }}
                    className={`w-8 h-8 flex items-center justify-center rounded-full transition-colors ${
                      isActive
                        ? "bg-white/20 hover:bg-white/30 text-white"
                        : "bg-gray-100 hover:bg-gray-200 text-gray-600"
                    }`}
                  >
                    {isActive ? (
                      <svg
                        className="w-4 h-4"
                        fill="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <rect x="6" y="4" width="4" height="16" rx="1" />
                        <rect x="14" y="4" width="4" height="16" rx="1" />
                      </svg>
                    ) : (
                      <svg
                        className="w-4 h-4"
                        fill="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path d="M8 5v14l11-7z" />
                      </svg>
                    )}
                  </button>
                </div>
              </div>
            );
          })}
        </div>

        {/* Task detail footer */}
        {selectedTask && (
          <div className="border-t border-gray-200 bg-white px-5 py-4">
            <h3 className="text-sm font-semibold text-gray-800">
              {selectedTask.name}
            </h3>
            <div className="flex items-center gap-2 mt-1">
              {selectedTask.elapsed_secs > selectedTask.budget_secs && (
                <span className="text-[10px] px-2 py-0.5 rounded-full font-semibold tracking-wide bg-red-100 text-red-700">
                  Time Exceeded
                </span>
              )}
              <p className="text-xs text-gray-400">
                Elapsed: {formatHms(selectedTask.elapsed_secs)} / Budget:{" "}
                {formatHms(selectedTask.budget_secs)}
              </p>
            </div>
          </div>
        )}

        {/* Today's Activity Summary */}
        <div className="border-t border-gray-200 bg-white px-4 py-3">
          <div className="text-xs uppercase tracking-wider text-gray-400 mb-2">
            Today's Activity
          </div>
          <div className="grid grid-cols-4 gap-3">
            <div className="text-center">
              <div className="text-lg font-mono font-semibold text-purple-700">
                {formatHms(totalDaySeconds)}
              </div>
              <div className="text-xs text-gray-400">Tracked</div>
            </div>
            <div className="text-center">
              <div className="text-lg font-mono font-semibold text-green-600">
                {formatHms(activity.active_secs)}
              </div>
              <div className="text-xs text-gray-400">Active</div>
            </div>
            <div className="text-center">
              <div className="text-lg font-mono font-semibold text-red-500">
                {formatHms(activity.idle_secs)}
              </div>
              <div className="text-xs text-gray-400">Idle</div>
            </div>
            <div className="text-center">
              <div
                className={`text-lg font-mono font-semibold ${activityColor}`}
              >
                {activity.activity_percent}%
              </div>
              <div className="text-xs text-gray-400">Activity</div>
            </div>
          </div>
        </div>

        {/* Status bar */}
        <div className="flex items-center justify-between px-4 py-2 bg-gray-100 border-t border-gray-200 text-xs text-gray-400">
          <span>
            Last updated on {new Date().toLocaleDateString("en-GB")},
            {new Date().toLocaleTimeString("en-GB")}
          </span>
          <div className="flex items-center gap-3">
            <button
              onClick={async () => {
                try {
                  const result = await invoke<Task[]>("refresh_tasks");
                  setTasks(result);
                } catch {
                  // API may be unreachable
                }
              }}
              className="flex items-center gap-1 text-gray-500 hover:text-purple-600 transition-colors"
              title="Refresh tasks from server"
            >
              <svg
                className="w-3.5 h-3.5"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                />
              </svg>
              Refresh
            </button>
            <span>v.1.0.0</span>
          </div>
        </div>
      </div>

      {/* Quit Confirmation Modal */}
      {idleEvent && (
        <div className="fixed inset-0 bg-black/30 flex items-start justify-center pt-24 z-50">
          <div className="w-full max-w-md overflow-hidden rounded-lg border border-gray-200 bg-white shadow-2xl">
            <div className="px-4 py-2.5 border-b border-gray-200 bg-gray-50 text-center text-[12px] font-semibold tracking-wide text-gray-700">
              Idle Time Alert
            </div>
            <div className="p-5 text-[12px] text-gray-700 space-y-4">
              <div className="leading-5">
                You have been idle for{" "}
                <span className="font-semibold text-gray-900">
                  {formatIdleDuration(idleEvent.idle_duration_secs)}
                </span>
                .
              </div>
              <div className="space-y-2 rounded-md border border-gray-200 bg-gray-50 px-3 py-3 text-[11px] text-gray-500">
                <div className="flex items-start justify-between gap-3">
                  Project:{" "}
                  <span className="text-right font-medium text-gray-800">
                    {idleEvent.task_name}
                  </span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span>Assign time to:</span>
                  <select
                    value={idleTaskId ?? ""}
                    onChange={(e) => setIdleTaskId(Number(e.target.value))}
                    disabled={idleEvent.tracking_active}
                    className="min-w-[190px] rounded-sm border border-gray-300 bg-white px-2 py-1.5 text-[11px] text-gray-700 disabled:bg-gray-100"
                  >
                    {tasks.map((task) => (
                      <option key={task.id} value={task.id}>
                        {task.name}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              {idleEvent.tracking_active && (
                <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-[11px] text-amber-700">
                  Idle time is still increasing. Actions unlock when you are
                  active again.
                </div>
              )}
              {!idleEvent.tracking_active && (
                <div className="text-[11px] text-gray-500">
                  Keep this time on the selected task or discard it.
                </div>
              )}
              <div className="flex items-center justify-end gap-2 pt-1">
                <button
                  onClick={handleDiscardIdleTime}
                  disabled={idleEvent.tracking_active}
                  className="rounded-sm border border-gray-300 bg-white px-3 py-1.5 text-[12px] text-gray-700 hover:bg-gray-50 disabled:bg-gray-100 disabled:text-gray-400"
                >
                  Discard idle time
                </button>
                <button
                  onClick={handleKeepIdleTime}
                  disabled={idleEvent.tracking_active || !idleTaskId}
                  className="rounded-sm border border-blue-600 bg-blue-600 px-3 py-1.5 text-[12px] text-white hover:bg-blue-700 disabled:border-blue-300 disabled:bg-blue-300"
                >
                  Keep idle time
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Quit Confirmation Modal */}
      {quitConfirmOpen && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg shadow-xl p-6 max-w-md w-full mx-4">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 bg-red-100 rounded-full flex items-center justify-center">
                <svg
                  className="w-5 h-5 text-red-600"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M12 9v2m0 4h.01M5.07 19h13.86c1.54 0 2.5-1.67 1.73-3L13.73 4c-.77-1.33-2.69-1.33-3.46 0L3.34 16c-.77 1.33.19 3 1.73 3z"
                  />
                </svg>
              </div>
              <div>
                <h2 className="text-lg font-semibold text-gray-800">
                  Quit the app?
                </h2>
                <p className="text-xs text-gray-400">
                  {quitHasRunning
                    ? "A timer is running. Stopping it will sync time to the server."
                    : "Your data is up to date. You can quit safely."}
                </p>
              </div>
            </div>

            {quitError && (
              <div className="bg-red-50 text-red-700 text-xs rounded-md px-3 py-2 mb-3">
                {quitError}
              </div>
            )}

            <div className="flex gap-3">
              <button
                onClick={() => setQuitConfirmOpen(false)}
                className="flex-1 bg-gray-200 text-gray-800 rounded-md py-2.5 text-sm font-medium hover:bg-gray-300 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmQuit}
                className="flex-1 bg-red-600 text-white rounded-md py-2.5 text-sm font-medium hover:bg-red-700 transition-colors"
              >
                {quitHasRunning ? "Stop and Quit" : "Quit"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Crash Recovery / Time Discarded Modal */}
      {crashRecoveryOpen && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg shadow-xl p-6 max-w-md w-full mx-4">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 bg-amber-100 rounded-full flex items-center justify-center">
                <svg
                  className="w-5 h-5 text-amber-600"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                  />
                </svg>
              </div>
              <div>
                <h2 className="text-lg font-semibold text-gray-800">
                  Time Verification
                </h2>
                <p className="text-xs text-gray-400">
                  Timestamp mismatch detected. Unwanted time has been discarded.
                </p>
              </div>
            </div>

            <div className="bg-amber-50 border border-amber-200 rounded-md px-4 py-3 mb-4">
              <p className="text-sm text-amber-800">
                The recorded time has been verified against the server. 
                Any discrepancy between local and server timestamps has been corrected 
                to ensure accurate time tracking.
                {crashRecoveredTaskId && (
                  <span className="block mt-2 font-medium">
                    Task ID: {crashRecoveredTaskId}
                  </span>
                )}
              </p>
            </div>

            <button
              onClick={() => {
                setCrashRecoveryOpen(false);
                setCrashRecoveredTaskId(null);
                refreshTasks();
              }}
              className="w-full bg-amber-600 text-white rounded-md py-2.5 text-sm font-medium hover:bg-amber-700 transition-colors"
            >
              Got it
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
