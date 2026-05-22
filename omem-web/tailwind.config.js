/** @type {import('tailwindcss').Config} */
module.exports = {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  prefix: "",
  safelist: [
    "bg-blue-500/10", "text-blue-600", "border-blue-500/30",
    "bg-green-500/10", "text-green-600", "border-green-500/30",
    "bg-rose-500/10", "text-rose-600", "border-rose-500/30",
    "bg-violet-500/10", "text-violet-600", "border-violet-500/30",
    "bg-red-500/10", "text-red-600", "border-red-500/30",
    "bg-orange-500/10", "text-orange-600", "border-orange-500/30",
    "bg-indigo-500/10", "text-indigo-600", "border-indigo-500/30",
    "bg-pink-500/10", "text-pink-600", "border-pink-500/30",
    "bg-slate-500/10", "text-slate-600", "border-slate-500/30",
    "bg-amber-500/10", "text-amber-600", "border-amber-500/30",
    "bg-cyan-500/10", "text-cyan-600", "border-cyan-500/30",
    "bg-emerald-500/10", "text-emerald-600", "border-emerald-500/30",
    "bg-teal-500/10", "text-teal-600", "border-teal-500/30",
    "bg-sky-500/10", "text-sky-600", "border-sky-500/30",
    "bg-fuchsia-500/10", "text-fuchsia-600", "border-fuchsia-500/30",
    "bg-lime-500/10", "text-lime-600", "border-lime-500/30",
    "bg-yellow-500/10", "text-yellow-600", "border-yellow-500/30",
    "bg-blue-100", "text-blue-700", "border-blue-200",
    "bg-emerald-50", "text-emerald-700", "border-emerald-200",
    "bg-amber-100", "text-amber-700", "border-amber-200",
    "hover:bg-blue-100", "hover:bg-emerald-50", "hover:bg-amber-100",
    "prose", "prose-sm", "dark:prose-invert", "max-w-none",
  ],
  theme: {
    container: {
      center: true,
      padding: "2rem",
      screens: {
        "2xl": "1400px",
      },
    },
    extend: {
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      keyframes: {
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: "0" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
    },
  },
  plugins: [require("tailwindcss-animate"), require("@tailwindcss/typography")],
}
