// Mirrors the inline `tailwind.config` previously embedded in src/auth.rs
// at line 197 — used by one specific auth page state. See
// tailwind/README.md for why there are 4 of these instead of 1.
module.exports = {
  content: ["../src/**/*.rs", "../crates/**/*.rs"],
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "error": "#ba1a1a", "surface-container-low": "#f5f3f3", "surface-container-high": "#e9e8e7",
        "surface-container-highest": "#e3e2e2", "on-surface-variant": "#444748"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "margin-desktop": "32px", "xs": "4px", "xl": "40px", "margin-mobile": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "code": ["13px", { lineHeight: "1.4", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }]
      }
    }
  },
  plugins: [
    require("@tailwindcss/forms"),
    require("@tailwindcss/container-queries"),
  ],
};
