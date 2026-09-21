var calendar;
var editingEventId = null;
var cfg = window.__WEBVIEW_DATA__.js_config;

function toLocalInputValue(dateStr) {
  if (!dateStr) return '';
  var d = new Date(dateStr);
  var pad = function(n) { return String(n).padStart(2, '0'); };
  return d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate()) + 'T' + pad(d.getHours()) + ':' + pad(d.getMinutes());
}

function resetOptionalModalFields() {
  document.getElementById('modal-field-location').value = '';
  document.getElementById('modal-field-notes').value = '';
  document.getElementById('modal-field-priority').value = '';
  document.getElementById('modal-field-busy').value = '';
  document.getElementById('modal-field-reminder').value = '';
  document.getElementById('modal-field-travel').value = '';
  document.getElementById('modal-repeat-display').classList.add('hidden');
  document.getElementById('modal-attendees-display').classList.add('hidden');
}

function openCreateModal(startStr, endStr, allDay) {
  editingEventId = null;
  document.getElementById('modal-title').textContent = cfg.modalTitleAdd;
  document.getElementById('modal-field-title').value = '';
  document.getElementById('modal-field-start').value = startStr ? toLocalInputValue(startStr) : '';
  document.getElementById('modal-field-end').value = endStr ? toLocalInputValue(endStr) : '';
  document.getElementById('modal-field-allday').checked = !!allDay;
  resetOptionalModalFields();
  document.getElementById('modal-notion-link').classList.add('hidden');
  document.getElementById('modal-delete-btn').classList.add('hidden');
  showModal();
}

function openEditModal(info) {
  editingEventId = info.event.id;
  document.getElementById('modal-title').textContent = cfg.modalTitleEdit;
  document.getElementById('modal-field-title').value = info.event.title;
  document.getElementById('modal-field-start').value = toLocalInputValue(info.event.startStr);
  document.getElementById('modal-field-end').value = info.event.endStr ? toLocalInputValue(info.event.endStr) : '';
  document.getElementById('modal-field-allday').checked = info.event.allDay;
  resetOptionalModalFields();
  var props = info.event.extendedProps;
  document.getElementById('modal-field-location').value = props.location || '';
  document.getElementById('modal-field-notes').value = props.notes || '';
  document.getElementById('modal-field-priority').value = props.priority || '';
  document.getElementById('modal-field-busy').value = (props.busy === true) ? 'true' : (props.busy === false) ? 'false' : '';
  document.getElementById('modal-field-reminder').value = props.reminderMinutes || '';
  document.getElementById('modal-field-travel').value = props.travelMinutes || '';
  if (props.repeatRule) {
    var repeatEl = document.getElementById('modal-repeat-display');
    repeatEl.textContent = cfg.repeatDisplayPrefix + props.repeatRule;
    repeatEl.classList.remove('hidden');
  }
  if (props.attendees && props.attendees.length) {
    var attendeesEl = document.getElementById('modal-attendees-display');
    attendeesEl.textContent = cfg.attendeesDisplayPrefix + props.attendees.join(', ');
    attendeesEl.classList.remove('hidden');
  }
  var notionUrl = info.event.extendedProps.notionUrl;
  var link = document.getElementById('modal-notion-link');
  if (notionUrl) {
    link.href = notionUrl;
    link.classList.remove('hidden');
    link.classList.add('inline-flex');
  } else {
    link.classList.add('hidden');
  }
  document.getElementById('modal-delete-btn').classList.remove('hidden');
  showModal();
}

function showModal() {
  var backdrop = document.getElementById('modal-backdrop');
  backdrop.classList.remove('hidden');
  backdrop.classList.add('flex');
}

function closeModal() {
  var backdrop = document.getElementById('modal-backdrop');
  backdrop.classList.add('hidden');
  backdrop.classList.remove('flex');
}

function saveFromModal() {
  var title = document.getElementById('modal-field-title').value.trim();
  if (!title) { alert(cfg.alertEnterTitle); return; }
  var allDay = document.getElementById('modal-field-allday').checked;
  var startVal = document.getElementById('modal-field-start').value;
  var endVal = document.getElementById('modal-field-end').value;
  if (!startVal) { alert(cfg.alertPickStart); return; }
  var localStartToUtcIso = allDay ? startVal.slice(0, 10) : new Date(startVal).toISOString();
  var localEndToUtcIso = endVal ? (allDay ? endVal.slice(0, 10) : new Date(endVal).toISOString()) : null;
  var start = localStartToUtcIso;
  var end = localEndToUtcIso;

  var payload = { title: title, start: start, end: end };
  var location = document.getElementById('modal-field-location').value.trim();
  if (location) payload.location = location;
  var notes = document.getElementById('modal-field-notes').value.trim();
  if (notes) payload.notes = notes;
  var priority = document.getElementById('modal-field-priority').value;
  if (priority) payload.priority = parseInt(priority, 10);
  var busy = document.getElementById('modal-field-busy').value;
  if (busy) payload.busy = (busy === 'true');
  var reminder = document.getElementById('modal-field-reminder').value;
  if (reminder) payload.reminder_minutes = parseInt(reminder, 10);
  var travel = document.getElementById('modal-field-travel').value.trim();
  if (travel) payload.travel_minutes = parseInt(travel, 10);

  if (editingEventId) {
    fetch(cfg.eventsUrl + '/' + encodeURIComponent(editingEventId), {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload)
    }).then(function(r) {
      if (!r.ok) { alert(r.status === 429 ? cfg.alertQuotaExceeded : cfg.alertUpdateFailed); return; }
      closeModal();
      calendar.refetchEvents();
    });
  } else {
    fetch(cfg.eventsUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload)
    }).then(function(r) {
      if (!r.ok) { alert(r.status === 429 ? cfg.alertQuotaExceeded : cfg.alertCreateFailed); return; }
      closeModal();
      calendar.refetchEvents();
    });
  }
}

function deleteFromModal() {
  if (!editingEventId) return;
  fetch(cfg.eventsUrl + '/' + encodeURIComponent(editingEventId), { method: 'DELETE' })
    .then(function(r) {
      if (!r.ok) { alert(cfg.alertDeleteFailed); return; }
      closeModal();
      calendar.refetchEvents();
    });
}

var deleteBtnConfirming = false;
var deleteBtnResetTimer = null;
function handleDeleteClick() {
  var btn = document.getElementById('modal-delete-btn');
  if (deleteBtnConfirming) {
    deleteBtnConfirming = false;
    clearTimeout(deleteBtnResetTimer);
    btn.textContent = cfg.deleteBtn;
    deleteFromModal();
  } else {
    deleteBtnConfirming = true;
    btn.textContent = cfg.confirmDeleteEvent;
    deleteBtnResetTimer = setTimeout(function() {
      deleteBtnConfirming = false;
      btn.textContent = cfg.deleteBtn;
    }, 3000);
  }
}

document.addEventListener('DOMContentLoaded', function() {
  var calendarEl = document.getElementById('calendar');
  calendar = new FullCalendar.Calendar(calendarEl, {
    initialView: 'dayGridMonth',
    headerToolbar: { left: 'prev,next today', center: 'title', right: 'dayGridMonth,timeGridWeek,listWeek' },
    selectable: true,
    editable: true,
    events: cfg.eventsUrl,

    select: function(info) {
      openCreateModal(info.startStr, info.endStr, info.allDay);
      calendar.unselect();
    },

    eventClick: function(info) {
      openEditModal(info);
    },

    eventDrop: function(info) { patchEventDates(info); },
    eventResize: function(info) { patchEventDates(info); },
  });

  function patchEventDates(info) {
    fetch(cfg.eventsUrl + '/' + encodeURIComponent(info.event.id), {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        start: info.event.startStr,
        end: info.event.endStr || null
      })
    }).then(function(r) {
      if (!r.ok) { alert(cfg.alertUpdateDateFailed); info.revert(); }
    });
  }

  calendar.render();
});
