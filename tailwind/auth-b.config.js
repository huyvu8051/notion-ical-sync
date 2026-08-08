// Mirrors the inline `tailwind.config` previously embedded in src/auth.rs
// at lines 666 and 906 (two page states, identical config). See
// tailwind/README.md for why there are 4 of these instead of 1.
module.exports = {
  content: ["../src/**/*.rs", "../crates/**/*.rs"],
  theme: {
    extend: {
      colors: {
        "outline-variant": "#c4c7c7", "outline": "#747878", "on-surface": "#1b1c1c",
        "primary": "#000000", "on-primary": "#ffffff", "background": "#fbf9f9", "surface": "#fbf9f9",
        "secondary": "#0058be", "secondary-container": "#2170e4",
        "surface-container-low": "#f5f3f3", "surface-container": "#efeded"
      },
      spacing: { "md": "16px", "lg": "24px", "sm": "8px", "margin-desktop": "32px", "xs": "4px", "xl": "40px", "margin-mobile": "16px", "gutter": "16px" },
      fontFamily: { "sans": ["Inter"], "code": ["Geist"] },
      fontSize: {
        "display": ["32px", { lineHeight: "1.2", letterSpacing: "-0.02em", fontWeight: "600" }],
        "h1": ["24px", { lineHeight: "1.3", letterSpacing: "-0.015em", fontWeight: "600" }],
        "h2": ["20px", { lineHeight: "1.4", letterSpacing: "-0.01em", fontWeight: "600" }],
        "h3": ["16px", { lineHeight: "1.5", letterSpacing: "-0.01em", fontWeight: "600" }],
        "body-lg": ["16px", { lineHeight: "1.6", fontWeight: "400" }],
        "body-md": ["14px", { lineHeight: "1.5", fontWeight: "400" }],
        "label-md": ["13px", { lineHeight: "1", letterSpacing: "0.02em", fontWeight: "500" }],
        "code": ["13px", { lineHeight: "1.4", fontWeight: "400" }]
      }
    }
  },
  plugins: [
    require("@tailwindcss/forms"),
    require("@tailwindcss/container-queries"),
  ],
};
