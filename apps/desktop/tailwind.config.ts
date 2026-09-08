import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        canvas: "#090B0E",
        surface1: "#0F1318",
        surface2: "#151B22",
        hairline: "#26303A",
        ink: "#E7EDF3",
        muted: "#8F9BA8",
        amber: "#F3A83B",
        positive: "#46D39A",
        negative: "#FF667A",
        info: "#63A7FF",
        unknown: "#8B7E9E",
      },
      fontFamily: {
        ui: ["Inter Variable", "Inter", "-apple-system", "BlinkMacSystemFont", "sans-serif"],
        mono: ["IBM Plex Mono", "ui-monospace", "SFMono-Regular", "monospace"],
      },
      fontSize: {
        label: ["12px", { lineHeight: "16px" }],
        body: ["13px", { lineHeight: "18px" }],
        body14: ["14px", { lineHeight: "20px" }],
        header: ["16px", { lineHeight: "20px" }],
        value: ["24px", { lineHeight: "28px" }],
      },
      borderRadius: {
        DEFAULT: "6px",
        panel: "6px",
      },
      spacing: {
        rail: "32px",
        nav: "56px",
        "nav-open": "184px",
        foot: "24px",
      },
      boxShadow: {
        trail: "0 0 0 1px #F3A83B33",
      },
    },
  },
  plugins: [],
} satisfies Config;
