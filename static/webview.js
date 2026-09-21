var calendar;

document.addEventListener('DOMContentLoaded', function() {
  var cfg = window.__WEBVIEW_DATA__.js_config;
  var calendarEl = document.getElementById('calendar');
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

  window.webview_refetch_calendar_events = function() {
    calendar.refetchEvents();
  };
});
