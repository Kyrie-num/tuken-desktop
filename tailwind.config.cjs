/** @type {import('tailwindcss').Config} */
module.exports = {
  darkMode: "class",
  content: ["./index.html", "./preview.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // SUSTech VI colours; see src/styles/index.css for both themes.
        ink: "rgb(var(--ink) / <alpha-value>)",
        orange: "rgb(var(--orange) / <alpha-value>)",
        cyan: "rgb(var(--cyan) / <alpha-value>)",
        "cyan-ink": "rgb(var(--cyan-ink) / <alpha-value>)",
        danger: "rgb(var(--danger) / <alpha-value>)",
        glass: "rgb(var(--glass) / <alpha-value>)",
        base: "rgb(var(--base) / <alpha-value>)",
      },
      fontFamily: {
        sans: [
          "Segoe UI Variable Text",
          "Segoe UI",
          "Microsoft YaHei UI",
          "PingFang SC",
          "system-ui",
          "sans-serif",
        ],
        display: [
          "Segoe UI Variable Display",
          "Segoe UI",
          "Microsoft YaHei UI",
          "PingFang SC",
          "system-ui",
          "sans-serif",
        ],
      },
      fontSize: {
        caption: ["12px", "18px"],
        small: ["13px", "20px"],
        body: ["14px", "22px"],
        strong: ["15px", "22px"],
        section: ["17px", "24px"],
        title: ["24px", "30px"],
        hero: ["48px", "52px"],
      },
      borderRadius: {
        glass: "24px",
        tile: "18px",
        field: "14px",
      },
      transitionTimingFunction: {
        liquid: "cubic-bezier(.22,.9,.24,1)",
      },
    },
  },
  plugins: [],
};
