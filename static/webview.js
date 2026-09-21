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

// FullCalendar's core bundle and locale plugin are two separate <script>
// tags loaded here (not via leptos_meta::Script) because leptos_meta
// inserts <script> elements into a live document on client-side SPA
// navigation, where dynamically-created scripts execute in load-finish
// order rather than document order — the locale plugin can then run before
// FullCalendar itself is defined. Chaining them explicitly here guarantees
// order regardless of navigation path.
function ensureFullCalendarLoaded() {
  if (window.FullCalendar) {
    return Promise.resolve();
  }
  if (!fullCalendarLoadPromise) {
    fullCalendarLoadPromise = loadScript('https://cdn.jsdelivr.net/npm/fullcalendar@6.1.15/index.global.min.js')
      .then(function() {
        return loadScript('https://cdn.jsdelivr.net/npm/@fullcalendar/core@6.1.15/locales/vi.global.min.js');
      });
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
  ensureFullCalendarLoaded().then(function() {
    var calendarEl = document.getElementById('calendar');
    // Route may have already navigated away (calendar.destroy() already ran
    // and unmounted #calendar) by the time the CDN scripts finish loading.
    if (!calendarEl) { return; }
    calendar = new FullCalendar.Calendar(calendarEl, {
      initialView: 'dayGridMonth',
      locale: cfg.locale,
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

// The bootstrap inline script (crates/app/src/webview.rs's
// WEBVIEW_JS_BOOTSTRAP) queues at most one pending init call in
// window.__webviewPendingConfig while this file is still loading, since a
// destroy (route-leave) after an init both just overwrite the same slot —
// last write wins, matching "only the currently-mounted page's calendar
// should end up initialized". Replay it now that the real functions above
// are in place.
if (window.__webviewPendingConfig) {
  var pendingConfig = window.__webviewPendingConfig;
  window.__webviewPendingConfig = null;
  window.webview_init_calendar(pendingConfig);
}
