/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        canvas: "#090B0E",
        s1: "#0F1318",
        s2: "#151B22",
        hair: "#26303A",
        ink: "#E7EDF3",
        mute: "#8F9BA8",
        amber: "#F3A83B",
        pos: "#46D39A",
        neg: "#FF667A",
        info: "#63A7FF",
        unk: "#8B7E9E",
      },
      fontFamily: {
        ui: ["Inter", "-apple-system", "system-ui", "sans-serif"],
        mono: ["IBM Plex Mono", "ui-monospace", "monospace"],
      },
    },
  },
  plugins: [],
};
