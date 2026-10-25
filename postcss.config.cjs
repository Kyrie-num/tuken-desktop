// A macOS build runs in WKWebView: Safari 16 (macOS 13) still needs the
// -webkit- prefixes that Windows' WebView2 doesn't. `tauri build` sets
// TAURI_ENV_PLATFORM; anything else gets the Windows output.
const mac = process.env.TAURI_ENV_PLATFORM === "darwin";

module.exports = {
  plugins: {
    tailwindcss: {},
    autoprefixer: mac ? { overrideBrowserslist: ["safari >= 16"] } : {},
  },
};
