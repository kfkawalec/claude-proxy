import type { Config } from "tailwindcss";

export default {
  content: ["./src/**/*.{ts,tsx}", "./index.html"],
  theme: {
    extend: {
      fontFamily: {
        sans: ["-apple-system", "BlinkMacSystemFont", "SF Pro Text", "Helvetica Neue", "sans-serif"],
      },
      colors: {
        accent: {
          DEFAULT: "#007AFF",
          hover: "#0066D6",
        },
        surface: {
          primary: "rgba(246, 246, 246, 0.8)",
          secondary: "rgba(255, 255, 255, 0.6)",
          elevated: "rgba(255, 255, 255, 0.9)",
        },
        border: {
          DEFAULT: "rgba(0, 0, 0, 0.1)",
          strong: "rgba(0, 0, 0, 0.2)",
        },
      },
      borderRadius: {
        mac: "10px",
        "mac-sm": "6px",
        "mac-lg": "14px",
      },
      fontSize: {
        "mac-xs": ["11px", "14px"],
        "mac-sm": ["12px", "16px"],
        "mac-base": ["13px", "18px"],
        "mac-lg": ["15px", "20px"],
        "mac-xl": ["17px", "22px"],
      },
    },
  },
  darkMode: "media",
  plugins: [],
} satisfies Config;
