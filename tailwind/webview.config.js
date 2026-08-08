// Mirrors the inline `tailwind.config` previously embedded in
// src/webview.rs (calendar view page). Kept as its own config — see
// tailwind/README.md for why there are 4 of these instead of 1.
module.exports = {
  content: ["../src/**/*.rs", "../crates/**/*.rs"],
  theme: {
    extend: {
      colors: {
        "outline-variant": "#e5e5e5", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "secondary": "#3B82F6", "error": "#ba1a1a", "surface-container-low": "#f5f3f3",
        "on-surface-variant": "#444748"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "xs": "4px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "h1": ["24px", { lineHeight: "1.3", fontWeight: "600" }],
        "h2": ["20px", { lineHeight: "1.4", fontWeight: "600" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }]
      }
    }
  },
  plugins: [
    require("@tailwindcss/forms"),
  ],
};
