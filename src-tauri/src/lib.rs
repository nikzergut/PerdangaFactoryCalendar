use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

const STANDARD_NAME: &str = "5/2 (стандартный)";
const MAX_NAME_LEN: usize = 40;

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

#[derive(Serialize, Deserialize, Clone, Debug)]
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
    /// Ошибка чтения файла при запуске; пока она есть, данные не читаются и не пишутся
    load_error: Option<String>,
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

/// Читает график. Отсутствующий файл даёт пустое состояние.
/// Файл, который не разбирается или не содержит графиков, переименовывается в `broken`-копию.
/// Файл, который не удалось прочитать по другой причине, не трогается, а возвращается ошибка.
fn load(path: &Path) -> Result<Store, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Store::default()),
        Err(e) => {
            return Err(format!(
                "Не удалось прочитать {}: {e}. Файл не изменён, сохранение отключено.",
                path.display()
            ))
        }
    };
    match serde_json::from_slice::<Store>(&bytes) {
        Ok(store) if !store.schedules.is_empty() => Ok(store),
        _ => {
            let backup = broken_path(path);
            fs::rename(path, &backup).map_err(|e| {
                format!(
                    "Файл {} повреждён и не переименован: {e}. Сохранение отключено.",
                    path.display()
                )
            })?;
            Ok(Store::default())
        }
    }
}

/// Имя для копии повреждённого файла, которое не перезаписывает существующие копии.
fn broken_path(path: &Path) -> PathBuf {
    let plain = path.with_extension("broken.json");
    if !plain.exists() {
        return plain;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let mut n = 1;
    loop {
        let candidate = path.with_extension(format!("broken-{stamp}-{n}.json"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Пишет во временный файл и заменяет им основной, чтобы обрыв записи не портил данные.
fn save(path: &Path, store: &Store) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Меняет копию состояния, сохраняет её и только после успешной записи подменяет память.
fn update<T>(
    state: &AppState,
    change: impl FnOnce(&mut Store) -> Result<T, String>,
) -> Result<T, String> {
    if let Some(err) = &state.load_error {
        return Err(err.clone());
    }
    let mut guard = state.store.lock().map_err(|e| e.to_string())?;
    let mut next = guard.clone();
    let value = change(&mut next)?;
    save(&state.path, &next)?;
    *guard = next;
    Ok(value)
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Название не может быть пустым".into());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(format!("Название не длиннее {MAX_NAME_LEN} символов"));
    }
    Ok(name.to_string())
}

/// Названия графиков не повторяются без учёта регистра; `except_id` — график, который переименовывают.
fn ensure_unique_name(store: &Store, name: &str, except_id: Option<&str>) -> Result<(), String> {
    let lower = name.to_lowercase();
    let taken = store
        .schedules
        .iter()
        .any(|s| Some(s.id.as_str()) != except_id && s.name.to_lowercase() == lower);
    if taken {
        return Err(format!("График «{name}» уже есть"));
    }
    Ok(())
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
        days.insert(day.to_string(), pattern[offset % pattern.len()]);
        changed += 1;
    }
    Ok(changed)
}

fn add_schedule(store: &mut Store, name: &str) -> Result<Schedule, String> {
    let name = clean_name(name)?;
    ensure_unique_name(store, &name, None)?;
    let schedule = Schedule::new(&name);
    store.schedules.push(schedule.clone());
    Ok(schedule)
}

fn add_standard_schedule(store: &mut Store) -> Result<Schedule, String> {
    ensure_unique_name(store, STANDARD_NAME, None)?;
    let mut schedule = Schedule::new(STANDARD_NAME);
    for calendar in CALENDARS {
        fill_standard(&mut schedule.days, calendar);
    }
    store.schedules.push(schedule.clone());
    Ok(schedule)
}

fn rename_schedule_in(store: &mut Store, id: &str, name: &str) -> Result<(), String> {
    let name = clean_name(name)?;
    ensure_unique_name(store, &name, Some(id))?;
    find_schedule(store, id)?.name = name;
    Ok(())
}

fn remove_schedule(store: &mut Store, id: &str) -> Result<(), String> {
    if store.schedules.len() <= 1 {
        return Err("Нельзя удалить единственный график".into());
    }
    find_schedule(store, id)?;
    store.schedules.retain(|s| s.id != id);
    Ok(())
}

fn set_days_in(
    store: &mut Store,
    schedule_id: &str,
    dates: &[String],
    kind: Option<DayKind>,
) -> Result<(), String> {
    let keys = dates
        .iter()
        .map(|date| parse_date(date).map(|day| day.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let schedule = find_schedule(store, schedule_id)?;
    for key in keys {
        match kind {
            Some(kind) => schedule.days.insert(key, kind),
            None => schedule.days.remove(&key),
        };
    }
    Ok(())
}

fn repeat_pattern_in(
    store: &mut Store,
    schedule_id: &str,
    from: &str,
    to: &str,
) -> Result<usize, String> {
    let from = parse_date(from)?;
    let to = parse_date(to)?;
    if to < from {
        return Err("Конец выделения раньше начала".into());
    }
    repeat_selection(&mut find_schedule(store, schedule_id)?.days, from, to)
}

#[tauri::command]
fn list_schedules(state: State<'_, AppState>) -> Result<Vec<Schedule>, String> {
    if let Some(err) = &state.load_error {
        return Err(err.clone());
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    Ok(store.schedules.clone())
}

#[tauri::command]
fn create_schedule(name: String, state: State<'_, AppState>) -> Result<Schedule, String> {
    update(&state, |store| add_schedule(store, &name))
}

#[tauri::command]
fn create_standard_schedule(state: State<'_, AppState>) -> Result<Schedule, String> {
    update(&state, add_standard_schedule)
}

#[tauri::command]
fn rename_schedule(id: String, name: String, state: State<'_, AppState>) -> Result<(), String> {
    update(&state, |store| rename_schedule_in(store, &id, &name))
}

#[tauri::command]
fn delete_schedule(id: String, state: State<'_, AppState>) -> Result<(), String> {
    update(&state, |store| remove_schedule(store, &id))
}

#[tauri::command]
fn set_days(
    schedule_id: String,
    dates: Vec<String>,
    kind: Option<DayKind>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    update(&state, |store| set_days_in(store, &schedule_id, &dates, kind))
}

#[tauri::command]
fn repeat_pattern(
    schedule_id: String,
    from: String,
    to: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    update(&state, |store| repeat_pattern_in(store, &schedule_id, &from, &to))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("schedules.json");
            let (store, load_error) = match load(&path) {
                Ok(store) => (store, None),
                Err(err) => (Store::default(), Some(err)),
            };
            app.manage(AppState {
                store: Mutex::new(store),
                path,
                load_error,
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

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pfc-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
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

    #[test]
    fn missing_file_gives_default_store() {
        let dir = temp_dir("missing");
        let store = load(&dir.join("schedules.json")).unwrap();

        assert_eq!(store.schedules.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_renamed_to_broken_copy() {
        let dir = temp_dir("corrupt");
        let path = dir.join("schedules.json");
        fs::write(&path, "{не json").unwrap();

        let store = load(&path).unwrap();

        assert_eq!(store.schedules.len(), 1);
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(dir.join("schedules.broken.json")).unwrap(),
            "{не json"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn second_corruption_keeps_previous_backup() {
        let dir = temp_dir("second");
        let path = dir.join("schedules.json");
        fs::write(dir.join("schedules.broken.json"), "первая копия").unwrap();
        fs::write(&path, "вторая порча").unwrap();

        load(&path).unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("schedules.broken.json")).unwrap(),
            "первая копия"
        );
        let copies: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("schedules.broken-"))
            .collect();
        assert_eq!(copies.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_schedule_list_is_treated_as_corrupt() {
        let dir = temp_dir("empty");
        let path = dir.join("schedules.json");
        fs::write(&path, r#"{"schedules":[]}"#).unwrap();

        let store = load(&path).unwrap();

        assert_eq!(store.schedules.len(), 1);
        assert!(!path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_file_is_not_touched() {
        let dir = temp_dir("unreadable");
        let path = dir.join("schedules.json");
        fs::create_dir(&path).unwrap();

        assert!(load(&path).is_err());
        assert!(path.is_dir());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("schedules.json");
        let mut store = Store::default();
        add_schedule(&mut store, "Ночной").unwrap();

        save(&path, &store).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded.schedules.len(), 2);
        assert_eq!(loaded.schedules[1].name, "Ночной");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_save_keeps_memory_unchanged() {
        let dir = temp_dir("failed-save");
        let path = dir.join("schedules.json");
        fs::create_dir(&path).unwrap();
        let state = AppState {
            store: Mutex::new(Store::default()),
            path: path.clone(),
            load_error: None,
        };

        assert!(update(&state, |store| add_schedule(store, "Новый")).is_err());
        assert_eq!(state.store.lock().unwrap().schedules.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_error_blocks_all_changes() {
        let state = AppState {
            store: Mutex::new(Store::default()),
            path: PathBuf::from("unused.json"),
            load_error: Some("нет доступа".into()),
        };

        assert!(update(&state, |store| add_schedule(store, "Новый")).is_err());
        assert_eq!(state.store.lock().unwrap().schedules.len(), 1);
    }

    #[test]
    fn set_days_sets_and_clears() {
        let mut store = Store::default();
        let id = store.schedules[0].id.clone();
        let dates = vec!["2026-03-01".to_string(), "2026-03-02".to_string()];

        set_days_in(&mut store, &id, &dates, Some(DayKind::Night)).unwrap();
        assert_eq!(store.schedules[0].days.get("2026-03-02"), Some(&DayKind::Night));

        set_days_in(&mut store, &id, &dates[..1], None).unwrap();
        assert_eq!(store.schedules[0].days.get("2026-03-01"), None);
        assert_eq!(store.schedules[0].days.len(), 1);
    }

    #[test]
    fn last_schedule_cannot_be_removed() {
        let mut store = Store::default();
        let id = store.schedules[0].id.clone();

        assert!(remove_schedule(&mut store, &id).is_err());
        assert_eq!(store.schedules.len(), 1);
    }

    #[test]
    fn standard_schedule_is_not_duplicated() {
        let mut store = Store::default();

        add_standard_schedule(&mut store).unwrap();
        assert!(add_standard_schedule(&mut store).is_err());
        assert_eq!(store.schedules.len(), 2);
    }

    #[test]
    fn names_are_unique_and_limited() {
        let mut store = Store::default();
        add_schedule(&mut store, "Смена").unwrap();

        assert!(add_schedule(&mut store, "смена").is_err());
        assert!(add_schedule(&mut store, &"я".repeat(MAX_NAME_LEN + 1)).is_err());

        let own_id = store.schedules[1].id.clone();
        assert!(rename_schedule_in(&mut store, &own_id, "Смена").is_ok());
    }
}
