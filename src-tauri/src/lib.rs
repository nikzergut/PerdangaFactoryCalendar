use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

const STANDARD_NAME: &str = "5/2 (стандартный)";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum DayKind {
    /// Раньше называлось `work`; старые файлы читаются без потерь
    #[serde(alias = "work")]
    Day,
    Night,
    Off,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Schedule {
    id: String,
    name: String,
    /// Ключ — дата в формате YYYY-MM-DD; такие строки сортируются по календарю
    days: BTreeMap<String, DayKind>,
}

impl Schedule {
    fn new(name: &str) -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_default();
        Self {
            id,
            name: name.to_string(),
            days: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Store {
    schedules: Vec<Schedule>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schedules: vec![Schedule::new("Мой график")],
        }
    }
}

struct AppState {
    store: Mutex<Store>,
    path: PathBuf,
}

/// Нерабочие дни производственного календаря в формате (месяц, день).
/// Суббота и воскресенье выходные, кроме `working_weekends`.
/// Источник: xmlcalendar.ru; 2026–2027 сверены с КонсультантПлюс и Гарант.
struct YearCalendar {
    year: i32,
    days_off: &'static [(u32, u32)],
    working_weekends: &'static [(u32, u32)],
}

const CALENDARS: &[YearCalendar] = &[
    YearCalendar {
        year: 2025,
        days_off: &[
            (1, 1), (1, 2), (1, 3), (1, 6), (1, 7), (1, 8),
            (5, 1), (5, 2), (5, 8), (5, 9),
            (6, 12), (6, 13),
            (11, 3), (11, 4),
            (12, 31),
        ],
        working_weekends: &[(11, 1)],
    },
    YearCalendar {
        year: 2026,
        days_off: &[
            (1, 1), (1, 2), (1, 5), (1, 6), (1, 7), (1, 8), (1, 9),
            (2, 23),
            (3, 9),
            (5, 1), (5, 11),
            (6, 12),
            (11, 4),
            (12, 31),
        ],
        working_weekends: &[],
    },
    YearCalendar {
        year: 2027,
        days_off: &[
            (1, 1), (1, 4), (1, 5), (1, 6), (1, 7), (1, 8),
            (2, 22), (2, 23),
            (3, 8),
            (5, 3), (5, 10),
            (6, 14),
            (11, 4), (11, 5),
            (12, 31),
        ],
        working_weekends: &[(2, 20)],
    },
];

/// Праздничные нерабочие дни по Трудовому кодексу: одинаковые даты в каждом году.
struct Holiday {
    month: u32,
    day: u32,
    short: &'static str,
    name: &'static str,
}

const HOLIDAYS: &[Holiday] = &[
    Holiday { month: 1, day: 1, short: "Новый год", name: "Новогодние каникулы" },
    Holiday { month: 1, day: 2, short: "Новый год", name: "Новогодние каникулы" },
    Holiday { month: 1, day: 3, short: "Новый год", name: "Новогодние каникулы" },
    Holiday { month: 1, day: 4, short: "Новый год", name: "Новогодние каникулы" },
    Holiday { month: 1, day: 5, short: "Новый год", name: "Новогодние каникулы" },
    Holiday { month: 1, day: 6, short: "Новый год", name: "Новогодние каникулы" },
    Holiday { month: 1, day: 7, short: "Рождество", name: "Рождество Христово" },
    Holiday { month: 1, day: 8, short: "Новый год", name: "Новогодние каникулы" },
    Holiday { month: 2, day: 23, short: "День защитника", name: "День защитника Отечества" },
    Holiday { month: 3, day: 8, short: "Женский день", name: "Международный женский день" },
    Holiday { month: 5, day: 1, short: "Весна и труд", name: "Праздник Весны и Труда" },
    Holiday { month: 5, day: 9, short: "День Победы", name: "День Победы" },
    Holiday { month: 6, day: 12, short: "День России", name: "День России" },
    Holiday { month: 11, day: 4, short: "День единства", name: "День народного единства" },
];

#[derive(Serialize)]
struct DayLabel {
    text: String,
    title: String,
}

/// Подписи дат года: праздники и, для годов из `CALENDARS`, перенесённые выходные.
#[tauri::command]
fn calendar_labels(year: i32) -> BTreeMap<String, DayLabel> {
    let mut labels = BTreeMap::new();
    for ordinal in 1..=366 {
        let Some(date) = NaiveDate::from_yo_opt(year, ordinal) else {
            break;
        };
        if let Some(holiday) = HOLIDAYS
            .iter()
            .find(|h| h.month == date.month() && h.day == date.day())
        {
            labels.insert(
                date.to_string(),
                DayLabel {
                    text: holiday.short.to_string(),
                    title: holiday.name.to_string(),
                },
            );
        }
    }
    if let Some(calendar) = CALENDARS.iter().find(|c| c.year == year) {
        for &(month, day) in calendar.days_off {
            let holiday = HOLIDAYS.iter().any(|h| h.month == month && h.day == day);
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day).filter(|_| !holiday) {
                labels.insert(
                    date.to_string(),
                    DayLabel {
                        text: "Перенос".into(),
                        title: "Перенос выходного".into(),
                    },
                );
            }
        }
    }
    labels
}

#[cfg(test)]
mod label_tests {
    use super::*;

    #[test]
    fn labels_holidays_and_transfers() {
        let labels = calendar_labels(2026);

        assert_eq!(labels["2026-01-07"].text, "Рождество");
        assert_eq!(labels["2026-01-03"].text, "Новый год");
        assert_eq!(labels["2026-01-09"].text, "Перенос");
        assert_eq!(labels["2026-02-23"].text, "День защитника");
        assert_eq!(labels["2026-05-09"].text, "День Победы");
        assert!(!labels.contains_key("2026-04-30"));
    }

    #[test]
    fn holidays_without_transfer_data() {
        let labels = calendar_labels(2028);

        assert_eq!(labels["2028-05-09"].text, "День Победы");
        assert!(!labels.contains_key("2028-05-11"));
    }
}

fn load(path: &Path) -> Store {
    let Ok(text) = fs::read_to_string(path) else {
        return Store::default();
    };
    serde_json::from_str(&text).unwrap_or_else(|_| {
        let _ = fs::rename(path, path.with_extension("broken.json"));
        Store::default()
    })
}

fn save(path: &Path, store: &Store) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Название не может быть пустым".into());
    }
    Ok(name.to_string())
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| format!("Некорректная дата: {s}"))
}

fn find_schedule<'a>(store: &'a mut Store, id: &str) -> Result<&'a mut Schedule, String> {
    store
        .schedules
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| "График не найден".to_string())
}

fn fill_standard(days: &mut BTreeMap<String, DayKind>, calendar: &YearCalendar) {
    for ordinal in 1..=366 {
        let Some(date) = NaiveDate::from_yo_opt(calendar.year, ordinal) else {
            break;
        };
        let md = (date.month(), date.day());
        let off = match date.weekday() {
            Weekday::Sat | Weekday::Sun => !calendar.working_weekends.contains(&md),
            _ => calendar.days_off.contains(&md),
        };
        let kind = if off { DayKind::Off } else { DayKind::Day };
        days.insert(date.to_string(), kind);
    }
}

/// Повторяет цикл из выделенных дней `from..=to` от следующего дня до конца года `to`.
fn repeat_selection(
    days: &mut BTreeMap<String, DayKind>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<usize, String> {
    let pattern = from
        .iter_days()
        .take_while(|day| *day <= to)
        .map(|day| days.get(&day.to_string()).copied())
        .collect::<Option<Vec<_>>>()
        .ok_or("В выделении есть неразмеченные дни")?;

    let mut changed = 0;
    for (offset, day) in to
        .iter_days()
        .skip(1)
        .take_while(|day| day.year() == to.year())
        .enumerate()
    {
        days.insert(day.to_string(), pattern[(pattern.len() + offset) % pattern.len()]);
        changed += 1;
    }
    Ok(changed)
}

#[tauri::command]
fn list_schedules(state: State<'_, AppState>) -> Result<Vec<Schedule>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    Ok(store.schedules.clone())
}

#[tauri::command]
fn create_schedule(name: String, state: State<'_, AppState>) -> Result<Schedule, String> {
    let schedule = Schedule::new(&clean_name(&name)?);
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store.schedules.push(schedule.clone());
    save(&state.path, &store)?;
    Ok(schedule)
}

#[tauri::command]
fn create_standard_schedule(state: State<'_, AppState>) -> Result<Schedule, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    if store.schedules.iter().any(|s| s.name == STANDARD_NAME) {
        return Err(format!("График «{STANDARD_NAME}» уже есть"));
    }
    let mut schedule = Schedule::new(STANDARD_NAME);
    for calendar in CALENDARS {
        fill_standard(&mut schedule.days, calendar);
    }
    store.schedules.push(schedule.clone());
    save(&state.path, &store)?;
    Ok(schedule)
}

#[tauri::command]
fn rename_schedule(id: String, name: String, state: State<'_, AppState>) -> Result<(), String> {
    let name = clean_name(&name)?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    find_schedule(&mut store, &id)?.name = name;
    save(&state.path, &store)
}

#[tauri::command]
fn delete_schedule(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    if store.schedules.len() <= 1 {
        return Err("Нельзя удалить единственный график".into());
    }
    store.schedules.retain(|s| s.id != id);
    save(&state.path, &store)
}

#[tauri::command]
fn set_days(
    schedule_id: String,
    dates: Vec<String>,
    kind: Option<DayKind>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let keys = dates
        .iter()
        .map(|date| parse_date(date).map(|day| day.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let schedule = find_schedule(&mut store, &schedule_id)?;
    for key in keys {
        match kind {
            Some(kind) => schedule.days.insert(key, kind),
            None => schedule.days.remove(&key),
        };
    }
    save(&state.path, &store)
}

#[tauri::command]
fn repeat_pattern(
    schedule_id: String,
    from: String,
    to: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let from = parse_date(&from)?;
    let to = parse_date(&to)?;
    if to < from {
        return Err("Конец выделения раньше начала".into());
    }
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let changed = repeat_selection(&mut find_schedule(&mut store, &schedule_id)?.days, from, to)?;
    save(&state.path, &store)?;
    Ok(changed)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("schedules.json");
            let store = load(&path);
            app.manage(AppState {
                store: Mutex::new(store),
                path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_schedules,
            create_schedule,
            create_standard_schedule,
            rename_schedule,
            delete_schedule,
            set_days,
            repeat_pattern,
            calendar_labels,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка при запуске приложения");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn day_count(days: &BTreeMap<String, DayKind>) -> usize {
        days.values().filter(|kind| **kind == DayKind::Day).count()
    }

    #[test]
    fn old_work_values_are_read_as_day() {
        let json = r#"{"schedules":[{"id":"1","name":"Old","days":{"2026-01-05":"work","2026-01-06":"off"}}]}"#;
        let store: Store = serde_json::from_str(json).unwrap();
        let days = &store.schedules[0].days;

        assert_eq!(days.get("2026-01-05"), Some(&DayKind::Day));
        assert_eq!(days.get("2026-01-06"), Some(&DayKind::Off));
    }

    #[test]
    fn standard_2025_has_working_saturday() {
        let mut days = BTreeMap::new();
        fill_standard(&mut days, &CALENDARS[0]);

        assert_eq!(days.get("2025-01-03"), Some(&DayKind::Off));
        assert_eq!(days.get("2025-11-01"), Some(&DayKind::Day));
        assert_eq!(days.get("2025-11-03"), Some(&DayKind::Off));
        assert_eq!(day_count(&days), 247);
    }

    #[test]
    fn standard_2026_marks_holidays_and_transfers() {
        let mut days = BTreeMap::new();
        fill_standard(&mut days, &CALENDARS[1]);

        assert_eq!(days.len(), 365);
        assert_eq!(days.get("2026-01-01"), Some(&DayKind::Off));
        assert_eq!(days.get("2026-01-09"), Some(&DayKind::Off));
        assert_eq!(days.get("2026-02-23"), Some(&DayKind::Off));
        assert_eq!(days.get("2026-05-11"), Some(&DayKind::Off));
        assert_eq!(days.get("2026-04-30"), Some(&DayKind::Day));
        assert_eq!(day_count(&days), 247);
    }

    #[test]
    fn standard_2027_has_working_saturday() {
        let mut days = BTreeMap::new();
        fill_standard(&mut days, &CALENDARS[2]);

        assert_eq!(days.get("2027-02-20"), Some(&DayKind::Day));
        assert_eq!(days.get("2027-02-22"), Some(&DayKind::Off));
        assert_eq!(day_count(&days), 247);
    }

    #[test]
    fn repeat_continues_selection_cycle() {
        let mut days = BTreeMap::new();
        let pattern = [DayKind::Day, DayKind::Night, DayKind::Off, DayKind::Off];
        for (i, kind) in pattern.into_iter().enumerate() {
            days.insert(day(2026, 3, i as u32 + 1).to_string(), kind);
        }

        let changed = repeat_selection(&mut days, day(2026, 3, 1), day(2026, 3, 4)).unwrap();

        assert_eq!(changed, 302);
        assert_eq!(days.get("2026-03-05"), Some(&DayKind::Day));
        assert_eq!(days.get("2026-03-06"), Some(&DayKind::Night));
        assert_eq!(days.get("2026-03-07"), Some(&DayKind::Off));
        assert_eq!(days.get("2026-03-09"), Some(&DayKind::Day));
        assert_eq!(days.get("2026-12-31"), Some(&DayKind::Night));
    }

    #[test]
    fn repeat_rejects_unmarked_days() {
        let mut days = BTreeMap::new();
        days.insert(day(2026, 3, 1).to_string(), DayKind::Day);

        assert!(repeat_selection(&mut days, day(2026, 3, 1), day(2026, 3, 2)).is_err());
    }
}
