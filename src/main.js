const { invoke } = window.__TAURI__.core;

const MONTHS = [
  'Январь', 'Февраль', 'Март', 'Апрель', 'Май', 'Июнь',
  'Июль', 'Август', 'Сентябрь', 'Октябрь', 'Ноябрь', 'Декабрь',
];

const pad = (n) => String(n).padStart(2, '0');
const dateKey = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const startOfMonth = (d) => new Date(d.getFullYear(), d.getMonth(), 1);
const parseKey = (key) => {
  const [y, m, d] = key.split('-').map(Number);
  return new Date(y, m - 1, d);
};
const formatKey = (key) => key.split('-').reverse().join('.');
const rangeKeys = (from, to) => {
  const keys = [];
  for (let d = parseKey(from); dateKey(d) <= to; d.setDate(d.getDate() + 1)) {
    keys.push(dateKey(d));
  }
  return keys;
};

const el = {
  list: document.getElementById('schedule-list'),
  title: document.getElementById('schedule-title'),
  add: document.getElementById('add-schedule'),
  addStandard: document.getElementById('add-standard'),
  rename: document.getElementById('rename-schedule'),
  remove: document.getElementById('delete-schedule'),
  prev: document.getElementById('prev-month'),
  next: document.getElementById('next-month'),
  today: document.getElementById('today'),
  monthLabel: document.getElementById('month-label'),
  grid: document.getElementById('grid'),
  statDay: document.getElementById('stat-day'),
  statNight: document.getElementById('stat-night'),
  statOff: document.getElementById('stat-off'),
  statNone: document.getElementById('stat-none'),
  selectionBar: document.getElementById('selection-bar'),
  selectionInfo: document.getElementById('selection-info'),
  repeat: document.getElementById('repeat-selection'),
  clearSelection: document.getElementById('clear-selection'),
  menu: document.getElementById('context-menu'),
  dialog: document.getElementById('schedule-dialog'),
  dialogTitle: document.getElementById('dialog-title'),
  form: document.getElementById('schedule-form'),
  nameInput: document.getElementById('schedule-name'),
};

const state = {
  schedules: [],
  activeId: null,
  month: startOfMonth(new Date()),
  selection: null, // { scheduleId, from, to }
};

let anchor = null; // день, с которого началось выделение левой кнопкой
let dialogTarget = { mode: 'create', scheduleId: null };

const activeSchedule = () => state.schedules.find((s) => s.id === state.activeId) ?? null;
const selectionInView = () => (state.selection?.scheduleId === state.activeId ? state.selection : null);
const inSelection = (key) => {
  const sel = selectionInView();
  return sel !== null && key >= sel.from && key <= sel.to;
};

const labelCache = new Map(); // год -> { 'YYYY-MM-DD': { text, title } }
const pendingYears = new Set();

const labelFor = (key) => labelCache.get(Number(key.slice(0, 4)))?.[key] ?? null;

async function loadLabels(years) {
  const missing = years.filter((y) => !labelCache.has(y) && !pendingYears.has(y));
  if (missing.length === 0) return;
  missing.forEach((y) => pendingYears.add(y));
  try {
    const loaded = await Promise.all(missing.map((year) => invoke('calendar_labels', { year })));
    missing.forEach((year, i) => labelCache.set(year, loaded[i]));
  } finally {
    missing.forEach((y) => pendingYears.delete(y));
  }
  render();
}

async function reload() {
  state.schedules = await invoke('list_schedules');
  if (!activeSchedule()) state.activeId = state.schedules[0]?.id ?? null;
  render();
}

function render() {
  renderSidebar();
  renderHeader();
  renderCalendar();
  renderStats();
  renderSelection();
}

function renderSidebar() {
  el.list.replaceChildren(
    ...state.schedules.map((s) => {
      const btn = document.createElement('button');
      btn.className = 'schedule-item' + (s.id === state.activeId ? ' active' : '');
      btn.dataset.id = s.id;
      btn.title = 'Правая кнопка — переименовать или удалить';
      btn.textContent = s.name;
      btn.addEventListener('click', () => {
        state.activeId = s.id;
        render();
      });
      const li = document.createElement('li');
      li.append(btn);
      return li;
    }),
  );
}

function renderHeader() {
  el.title.textContent = activeSchedule()?.name ?? '';
  el.remove.disabled = state.schedules.length <= 1;
}

function updateCell(cell) {
  const key = cell.dataset.date;
  cell.dataset.kind = activeSchedule()?.days[key] ?? '';
  cell.classList.toggle('selected', inSelection(key));
}

function renderCalendar() {
  const year = state.month.getFullYear();
  const month = state.month.getMonth();
  const offset = (state.month.getDay() + 6) % 7; // неделя начинается с понедельника
  const cellCount = Math.ceil((offset + new Date(year, month + 1, 0).getDate()) / 7) * 7;
  const today = dateKey(new Date());

  loadLabels([year - 1, year, year + 1]).catch((err) => console.error(err));
  el.monthLabel.textContent = `${MONTHS[month]} ${year}`;
  el.grid.replaceChildren(
    ...Array.from({ length: cellCount }, (_, i) => {
      const date = new Date(year, month, i - offset + 1);
      const key = dateKey(date);

      const cell = document.createElement('div');
      cell.className = 'day';
      cell.dataset.date = key;
      if (date.getMonth() !== month) cell.classList.add('outside');
      if (key === today) cell.classList.add('today');

      const num = document.createElement('span');
      num.className = 'num';
      num.textContent = date.getDate();
      cell.append(num);

      const label = labelFor(key);
      if (label) {
        const holiday = document.createElement('span');
        holiday.className = 'holiday';
        holiday.textContent = label.text;
        cell.title = label.title;
        cell.append(holiday);
      }

      updateCell(cell);
      return cell;
    }),
  );
}

function renderStats() {
  const days = activeSchedule()?.days ?? {};
  const year = state.month.getFullYear();
  const month = state.month.getMonth();
  const prefix = `${year}-${pad(month + 1)}-`;

  const counts = { day: 0, night: 0, off: 0 };
  for (const [key, kind] of Object.entries(days)) {
    if (key.startsWith(prefix) && kind in counts) counts[kind]++;
  }

  el.statDay.textContent = counts.day;
  el.statNight.textContent = counts.night;
  el.statOff.textContent = counts.off;
  el.statNone.textContent = new Date(year, month + 1, 0).getDate() - counts.day - counts.night - counts.off;
}

function renderSelection() {
  const sel = selectionInView();
  el.selectionBar.hidden = sel === null;
  if (sel === null) return;
  const count = rangeKeys(sel.from, sel.to).length;
  el.selectionInfo.textContent = `Выделено ${count} дн.: ${formatKey(sel.from)} — ${formatKey(sel.to)}`;
}

function refreshBoard() {
  for (const cell of el.grid.children) updateCell(cell);
  renderStats();
  renderSelection();
}

function clearSelection() {
  state.selection = null;
  refreshBoard();
}

async function markDays(scheduleId, keys, kind) {
  const schedule = state.schedules.find((s) => s.id === scheduleId);
  if (!schedule) return;
  for (const key of keys) {
    if (kind) schedule.days[key] = kind;
    else delete schedule.days[key];
  }
  refreshBoard();

  try {
    await invoke('set_days', { scheduleId, dates: keys, kind });
  } catch (err) {
    alert(`Не удалось сохранить: ${err}`);
    await reload().catch((reloadErr) => alert(`Не удалось загрузить графики: ${reloadErr}`));
  }
}

async function repeatSelection() {
  const schedule = activeSchedule();
  const sel = selectionInView();
  if (!schedule || !sel) return;

  if (rangeKeys(sel.from, sel.to).some((key) => !schedule.days[key])) {
    alert('В выделении есть неразмеченные дни. Сначала отметьте все дни цикла.');
    return;
  }

  const year = sel.to.slice(0, 4);
  const overwritten = rangeKeys(sel.to, `${year}-12-31`)
    .slice(1)
    .filter((key) => schedule.days[key]).length;
  if (overwritten && !confirm(`Перезаписать существующие отметки до конца ${year} года (${overwritten} шт.)?`)) {
    return;
  }

  try {
    await invoke('repeat_pattern', { scheduleId: schedule.id, from: sel.from, to: sel.to });
    await reload();
  } catch (err) {
    alert(err);
  }
}

async function deleteSchedule(schedule) {
  if (!confirm(`Удалить график «${schedule.name}»?`)) return;
  try {
    await invoke('delete_schedule', { id: schedule.id });
    state.activeId = null;
    await reload();
  } catch (err) {
    alert(err);
  }
}

function openDialog(mode, schedule = null) {
  dialogTarget = { mode, scheduleId: schedule?.id ?? null };
  el.dialogTitle.textContent = mode === 'create' ? 'Новый график' : 'Переименовать график';
  el.nameInput.value = schedule?.name ?? '';
  el.dialog.showModal();
  el.nameInput.focus();
}

function openMenu(x, y, items) {
  el.menu.replaceChildren(
    ...items.map((item) => {
      const btn = document.createElement('button');
      btn.textContent = item.label;
      if (item.danger) btn.classList.add('danger');
      btn.addEventListener('click', () => {
        closeMenu();
        item.onClick();
      });
      return btn;
    }),
  );
  el.menu.hidden = false;
  el.menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - el.menu.offsetWidth - 8))}px`;
  el.menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - el.menu.offsetHeight - 8))}px`;
}

function closeMenu() {
  el.menu.hidden = true;
}

function shiftMonth(delta) {
  state.month = new Date(state.month.getFullYear(), state.month.getMonth() + delta, 1);
  render();
}

const LONG_PRESS_MS = 500;
const MOVE_TOLERANCE_PX = 10;
let press = null; // долгое нажатие пальцем: { x, y, timer }

function cancelPress() {
  if (press) clearTimeout(press.timer);
  press = null;
}

function startPress(e, onLongPress) {
  if (e.pointerType === 'mouse') return;
  cancelPress();
  const { clientX: x, clientY: y } = e;
  press = {
    x,
    y,
    timer: setTimeout(() => {
      press = null;
      onLongPress(x, y);
    }, LONG_PRESS_MS),
  };
}

function extendSelection(x, y) {
  if (anchor === null) return;
  const cell = document.elementFromPoint(x, y)?.closest('.day');
  if (!cell || !el.grid.contains(cell)) return;
  const [from, to] = [anchor, cell.dataset.date].sort();
  if (state.selection?.from === from && state.selection?.to === to) return;
  state.selection = { scheduleId: state.activeId, from, to };
  refreshBoard();
}

function openCellMenu(x, y, cell) {
  const schedule = activeSchedule();
  if (!cell || !schedule) return;

  const key = cell.dataset.date;
  const sel = selectionInView();
  const keys = sel && inSelection(key) ? rangeKeys(sel.from, sel.to) : [key];
  openMenu(x, y, [
    { label: 'День', onClick: () => markDays(schedule.id, keys, 'day') },
    { label: 'Ночь', onClick: () => markDays(schedule.id, keys, 'night') },
    { label: 'Выходной', onClick: () => markDays(schedule.id, keys, 'off') },
    { label: 'Очистить', onClick: () => markDays(schedule.id, keys, null) },
  ]);
}

function openScheduleMenu(x, y, schedule) {
  openMenu(x, y, [
    { label: 'Переименовать', onClick: () => openDialog('rename', schedule) },
    { label: 'Удалить', danger: true, onClick: () => deleteSchedule(schedule) },
  ]);
}

el.grid.addEventListener('pointerdown', (e) => {
  const cell = e.target.closest('.day');
  const schedule = activeSchedule();
  if (!cell || !schedule || e.button !== 0) return;
  e.preventDefault();
  anchor = cell.dataset.date;
  state.selection = { scheduleId: schedule.id, from: anchor, to: anchor };
  refreshBoard();
  startPress(e, (x, y) => {
    anchor = null;
    openCellMenu(x, y, cell);
  });
});

el.grid.addEventListener('contextmenu', (e) => {
  const cell = e.target.closest('.day');
  if (!cell || !activeSchedule()) return;
  e.preventDefault();
  openCellMenu(e.clientX, e.clientY, cell);
});

el.list.addEventListener('pointerdown', (e) => {
  const item = e.target.closest('.schedule-item');
  const schedule = state.schedules.find((s) => s.id === item?.dataset.id);
  if (!schedule) return;
  startPress(e, (x, y) => openScheduleMenu(x, y, schedule));
});

el.list.addEventListener('contextmenu', (e) => {
  const item = e.target.closest('.schedule-item');
  const schedule = state.schedules.find((s) => s.id === item?.dataset.id);
  if (!schedule) return;
  e.preventDefault();
  openScheduleMenu(e.clientX, e.clientY, schedule);
});

window.addEventListener('pointermove', (e) => {
  if (press && Math.hypot(e.clientX - press.x, e.clientY - press.y) > MOVE_TOLERANCE_PX) {
    cancelPress();
  }
  extendSelection(e.clientX, e.clientY);
});
window.addEventListener('pointerup', () => {
  anchor = null;
  cancelPress();
});
window.addEventListener('pointercancel', () => {
  anchor = null;
  cancelPress();
});
window.addEventListener('pointerdown', (e) => {
  if (!el.menu.contains(e.target)) closeMenu();
});
window.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape' || el.dialog.open) return;
  closeMenu();
  clearSelection();
});

el.prev.addEventListener('click', () => shiftMonth(-1));
el.next.addEventListener('click', () => shiftMonth(1));
el.today.addEventListener('click', () => {
  state.month = startOfMonth(new Date());
  render();
});

el.clearSelection.addEventListener('click', clearSelection);
el.repeat.addEventListener('click', repeatSelection);

el.add.addEventListener('click', () => openDialog('create'));
el.rename.addEventListener('click', () => openDialog('rename', activeSchedule()));
el.remove.addEventListener('click', () => {
  const schedule = activeSchedule();
  if (schedule) deleteSchedule(schedule);
});

el.addStandard.addEventListener('click', async () => {
  try {
    const created = await invoke('create_standard_schedule');
    state.activeId = created.id;
    await reload();
  } catch (err) {
    alert(err);
  }
});

el.form.addEventListener('submit', async (e) => {
  if (e.submitter?.value !== 'save') return;
  const name = el.nameInput.value.trim();
  try {
    if (dialogTarget.mode === 'create') {
      const created = await invoke('create_schedule', { name });
      state.activeId = created.id;
    } else {
      await invoke('rename_schedule', { id: dialogTarget.scheduleId, name });
    }
    await reload();
  } catch (err) {
    alert(err);
  }
});

if (matchMedia('(pointer: coarse)').matches) {
  document.getElementById('hint').textContent = 'Протяните, чтобы выделить · долгое нажатие — изменить день';
}

reload().catch((err) => alert(`Не удалось загрузить графики: ${err}`));
