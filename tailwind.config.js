/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        xp: {
          grey: "#ECE9D8",
          greyLight: "#F1EFE2",
          greyBorder: "#D2CFB9",
          greyShadow: "#808080",
          blue: "#245DD7",
          blueDark: "#002E99",
          blueLight: "#3198FF",
          titlebar: "#0050E6",
          green: "#107C10",
          greenDark: "#0A4D0A",
          greenLight: "#28B428",
          red: "#E72525",
          redDark: "#B81C1C",
          yellow: "#F2A900",
          inputBg: "#FFFFFF",
        }
      },
      fontFamily: {
        tahoma: ["Tahoma", "Geneva", "Verdana", "sans-serif"],
      }
    },
  },
  plugins: [],
}
