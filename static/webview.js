var calendar;
var fullCalendarLoadPromise = null;

function loadScript(src) {
  return new Promise(function(resolve, reject) {
    var s = document.createElement('script');
    s.src = src;
    s.onload = resolve;
    s.onerror = reject;
    document.head.appendChild(s);
  });
}

function loadFullCalendarCore() {
  if (window.FullCalendar) {
    return Promise.resolve();
  }
  if (!fullCalendarLoadPromise) {
    fullCalendarLoadPromise = loadScript('https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.js');
  }
  return fullCalendarLoadPromise;
}

function patchEventDates(cfg, info) {
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

window.webview_init_calendar = function(configJson) {
  var cfg = JSON.parse(configJson);
  loadFullCalendarCore().then(function() {
    var calendarEl = document.getElementById('calendar');
    if (!calendarEl) { return; }
    calendar = new FullCalendar.Calendar(calendarEl, {
      initialView: 'dayGridMonth',
      headerToolbar: { left: 'prev,next today', center: 'title', right: 'dayGridMonth,timeGridWeek,listWeek' },
      selectable: true,
      editable: true,
      events: cfg.eventsUrl,

      select: function(info) {
        window.webview_open_create_modal(info.startStr, info.endStr, info.allDay);
        calendar.unselect();
      },

      eventClick: function(info) {
        var props = info.event.extendedProps;
        window.webview_open_edit_modal(JSON.stringify({
          id: info.event.id,
          title: info.event.title,
          start: info.event.startStr,
          end: info.event.endStr || null,
          allDay: info.event.allDay,
          location: props.location || null,
          notes: props.notes || null,
          priority: props.priority || null,
          busy: props.busy,
          reminderMinutes: props.reminderMinutes || null,
          travelMinutes: props.travelMinutes || null,
          repeatRule: props.repeatRule || null,
          attendees: props.attendees || [],
          notionUrl: props.notionUrl || null,
        }));
      },

      eventDrop: function(info) { patchEventDates(cfg, info); },
      eventResize: function(info) { patchEventDates(cfg, info); },
    });

    calendar.render();
  });
};

window.webview_destroy_calendar = function() {
  if (calendar) {
    calendar.destroy();
    calendar = null;
  }
};

window.webview_refetch_calendar_events = function() {
  if (calendar) { calendar.refetchEvents(); }
};

function replayCalendarInitQueuedDuringLoad() {
  if (!window.__webviewPendingConfig) { return; }
  var pendingConfig = window.__webviewPendingConfig;
  window.__webviewPendingConfig = null;
  window.webview_init_calendar(pendingConfig);
}

replayCalendarInitQueuedDuringLoad();
