import { useEffect, useState } from "react";
import { windowMaterial } from "./api";

export type Theme = "light" | "dark" | "system";
const KEY = "tuken-theme";

/** Light unless the user picked dark or "follow the system". */
export function storedTheme(): Theme {
  const t = localStorage.getItem(KEY);
  return t === "dark" || t === "system" ? t : "light";
}

const systemDark = () =>
  typeof matchMedia === "function" &&
  matchMedia("(prefers-color-scheme: dark)").matches;

/** Sets the page theme and tints the window's material to match. */
export function applyTheme(theme: Theme) {
  const dark = theme === "dark" || (theme === "system" && systemDark());
  document.documentElement.classList.toggle("dark", dark);
  windowMaterial.matchTheme(dark, theme === "system").catch(() => {});
}

export function useTheme() {
  const [theme, setTheme] = useState<Theme>(storedTheme);
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system" || typeof matchMedia !== "function") return;
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const follow = () => applyTheme("system");
    mq.addEventListener("change", follow);
    return () => mq.removeEventListener("change", follow);
  }, [theme]);
  return [
    theme,
    (t: Theme) => {
      localStorage.setItem(KEY, t);
      setTheme(t);
    },
  ] as const;
}
