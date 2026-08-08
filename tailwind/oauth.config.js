// Mirrors the inline `tailwind.config` previously embedded in
// src/oauth.rs (Notion OAuth consent flow). See tailwind/README.md for
// why there are 4 of these instead of 1.
module.exports = {
  content: ["../src/**/*.rs", "../crates/**/*.rs"],
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "error": "#ba1a1a", "error-container": "#ffdad6", "on-error-container": "#93000a",
        "surface-container-low": "#f5f3f3", "surface-container-high": "#e9e8e7", "surface-container-highest": "#e3e2e2",
        "surface-container": "#efeded", "surface-container-lowest": "#ffffff",
        "on-surface-variant": "#444748", "secondary": "#0058be"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "xs": "4px", "margin-desktop": "32px", "margin-mobile": "16px", "xl": "40px", "gutter": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h3": ["16px", { lineHeight: "1.5", letterSpacing: "-0.01em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }]
      }
    }
  },
  plugins: [
    require("@tailwindcss/forms"),
    require("@tailwindcss/container-queries"),
  ],
};
