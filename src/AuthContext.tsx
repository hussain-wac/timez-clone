import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AuthUser {
  id: number;
  email: string;
  name: string | null;
  picture: string | null;
}

interface AuthResponse {
  access_token: string;
  user: AuthUser;
}

interface AuthContextType {
  isAuthenticated: boolean;
  user: AuthUser | null;
  accessToken: string | null;
  isLoading: boolean;
  loginViaBrowser: () => Promise<void>;
  logout: () => void;
}

const GOOGLE_CLIENT_ID = import.meta.env.VITE_GOOGLE_CLIENT_ID as string;
const GOOGLE_CLIENT_SECRET = import.meta.env.VITE_GOOGLE_CLIENT_SECRET as string;

const AuthContext = createContext<AuthContextType | null>(null);

export function useAuth(): AuthContextType {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [accessToken, setAccessToken] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // On mount, check for stored token
  useEffect(() => {
    const stored = localStorage.getItem("access_token");
    if (stored) {
      invoke<AuthUser>("validate_token", { token: stored })
        .then((u) => {
          setUser(u);
          setAccessToken(stored);
        })
        .catch(() => {
          localStorage.removeItem("access_token");
        })
        .finally(() => setIsLoading(false));
    } else {
      setIsLoading(false);
    }
  }, []);

  const loginViaBrowser = useCallback(async () => {
    // Start OAuth in background - don't wait for response
    await invoke<string>(
      "start_google_auth",
      { clientId: GOOGLE_CLIENT_ID, clientSecret: GOOGLE_CLIENT_SECRET },
    );
    // The actual login will be handled via events below
  }, []);

  // Listen for auth events
  useEffect(() => {
    let unlistenSuccess: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;

    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<AuthResponse>("auth-success", (event) => {
        const response = event.payload;
        setUser(response.user);
        setAccessToken(response.access_token);
        localStorage.setItem("access_token", response.access_token);
      }).then(fn => unlistenSuccess = fn);

      listen<string>("auth-error", (event) => {
        console.error("Auth error:", event.payload);
      }).then(fn => unlistenError = fn);
    });

    return () => {
      unlistenSuccess?.();
      unlistenError?.();
    };
  }, []);

  const logout = useCallback(() => {
    invoke("logout").catch(() => {});
    setUser(null);
    setAccessToken(null);
    localStorage.removeItem("access_token");
  }, []);

  return (
    <AuthContext.Provider
      value={{
        isAuthenticated: !!user,
        user,
        accessToken,
        isLoading,
        loginViaBrowser,
        logout,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}
