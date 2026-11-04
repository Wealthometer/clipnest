import { useCallback, useEffect, useState } from "react";
import { api, errorText, on, type AppEvent, type AppInfo } from "./api.ts";
import { toast } from "./ui/kit.tsx";

/** Loads something from Rust and reloads it whenever one of `events` fires. */
export function useLoad<T>(load: () => Promise<T>, events: AppEvent[], deps: unknown[] = []): [T | undefined, () => void] {
  const [value, setValue] = useState<T>();
  const [tick, setTick] = useState(0);
  const reload = useCallback(() => setTick((t) => t + 1), []);
  useEffect(() => {
    let alive = true;
    load().then(
      (v) => alive && setValue(v),
      (e) => alive && toast(errorText(e), "error"),
    );
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, ...deps]);
  useEffect(() => {
    // Bursts of copies arrive close together; reload once.
    let t: ReturnType<typeof setTimeout> | undefined;
    const offs = events.map((ev) =>
      on(ev, () => {
        clearTimeout(t);
        t = setTimeout(reload, 80);
      }),
    );
    return () => {
      clearTimeout(t);
      offs.forEach((o) => o());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reload, events.join()]);
  return [value, reload];
}

export function useInfo(): AppInfo | undefined {
  const [info] = useLoad(api.info, ["state-changed"]);
  return info;
}

/** Runs a call, showing its error as a toast; returns undefined on failure. */
export async function attempt<T>(p: Promise<T>, ok?: string): Promise<T | undefined> {
  try {
    const r = await p;
    if (ok) toast(ok);
    return r;
  } catch (e) {
    toast(errorText(e), "error");
    return undefined;
  }
}

/** Applies the theme setting to the page. */
export function useTheme(theme: "system" | "dark" | "light" | undefined) {
  useEffect(() => {
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => root.setAttribute("data-theme", theme === "system" || !theme ? (media.matches ? "dark" : "light") : theme);
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}
