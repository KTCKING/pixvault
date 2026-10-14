import { createSignal, createEffect } from "solid-js";

export type Theme = "light" | "dark";

const [theme, setTheme] = createSignal<Theme>(
  (localStorage.getItem("pixvault-theme") as Theme) || "light"
);

// Apply theme class to document
createEffect(() => {
  const current = theme();
  document.documentElement.classList.remove("light", "dark");
  document.documentElement.classList.add(current);
  localStorage.setItem("pixvault-theme", current);
});

export function useTheme() {
  const toggle = () => {
    setTheme((prev) => (prev === "light" ? "dark" : "light"));
  };

  return { theme, setTheme, toggle };
}