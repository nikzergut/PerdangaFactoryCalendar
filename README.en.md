# PerdangaFactoryCalendar

[Русская версия](README.md)

A desktop app for building work schedules. Each day can be marked as "Day", "Night" or "Day off". Public holidays and transferred days off are labeled according to the Russian production calendar. The interface is in Russian.

## Features

- **Multiple schedules.** On first launch, a schedule named «Мой график» ("My schedule") is created. You can create, rename and delete schedules. A schedule can only be deleted if more than one exists.
- **Standard 5/2 schedule.** The «+ 5/2 (стандартный)» button fills in 2025–2027 using the production calendar: Saturdays and Sundays are days off, except the working Saturdays of 1 November 2025 and 20 February 2027.
- **Monthly calendar.** Weeks start on Monday. Use ‹ and › to switch months, and «Сегодня» ("Today") to return to the current month.
- **Marking days.** Hold the left mouse button and drag to select several days. Right-click opens a menu with «День» (Day), «Ночь» (Night), «Выходной» (Day off) and «Очистить» (Clear). If you right-click a selected day, the action applies to the whole selection. Press Esc to clear the selection and close the menu.
- **Repeating a pattern.** The «Размножить до конца года» ("Repeat to end of year") button repeats the selected pattern of days, nights and days off from the day after the selection until December 31. All selected days must be marked first. Before overwriting existing marks, the app asks for confirmation.
- **Monthly statistics.** Counts of day, night and day-off days, plus days without a mark.
- **Holidays and transfers.** Shown in the calendar cells. Hover over a label to see the full name. Transferred days off are available for 2025–2027 only.

## Data storage

Changes are saved immediately to `schedules.json` in the app's data folder. On Windows this is `%APPDATA%\ru.example.graphics\`. If the file is corrupted, it is renamed to `schedules.broken.json`, and the app starts with a new empty «Мой график» schedule.

## Tech stack

- [Tauri 2](https://tauri.app/): application shell. The logic is written in Rust, and the interface is rendered with WebView2.
- Frontend: plain HTML, CSS and JavaScript, with no framework and no bundler.
- Rust libraries: chrono (dates), serde and serde_json (data storage).

## Project structure

```
.
├── src/                  interface: index.html, main.js, styles.css
└── src-tauri/
    ├── src/
    │   ├── lib.rs        schedules, production calendar, Tauri commands, tests
    │   └── main.rs       entry point
    ├── capabilities/     window permissions
    ├── icons/            app icons
    └── tauri.conf.json   Tauri settings
```

## Development setup

You will need:

- [Node.js](https://nodejs.org/) (LTS) with npm
- [Rust](https://www.rust-lang.org/tools/install) via rustup
- Tauri prerequisites for Windows: Microsoft C++ Build Tools with the "Desktop development with C++" workload, and WebView2. See https://tauri.app/start/prerequisites/

```bash
npm install
npm run dev
```

## Build

```bash
npm run build
```

The installers are written to `src-tauri/target/release/bundle/`: the MSI goes to `msi/` and the `.exe` installer goes to `nsis/`.

## Tests

```bash
cd src-tauri
cargo test
```

## Calendar data

Days off, transfers and working Saturdays for the standard schedule and the transfer labels are stored in the `CALENDARS` constant in `src-tauri/src/lib.rs`. To add a new year, add it there.
